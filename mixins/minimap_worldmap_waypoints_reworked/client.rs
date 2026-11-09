// SPDX-License-Identifier: GPL-3.0-only
// Engine adapter: this module is injected under game, never into the standalone library.
use super::Game;
use runtime::{Language, language, tr};

macro_rules! localized_format {
    ($en:literal, $ru:literal, $fr:literal, $es:literal, $de:literal $(, $arg:expr)* $(,)?) => {
        match language() {
            Language::Russian => format!($ru $(, $arg)*),
            Language::French => format!($fr $(, $arg)*),
            Language::Spanish => format!($es $(, $arg)*),
            Language::German => format!($de $(, $arg)*),
            Language::English => format!($en $(, $arg)*),
        }
    };
}

use super::minimap_worldmap_waypoints_reworked::{
    self as runtime, Dimension, Waypoint,
    maps::{self, Layer, Portal, View},
};
use horizon_client::{chunk::Chunk, extras::Corner, extras_hud::Corners, hud::HudBatch};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock, Weak},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Map,
    Waypoints,
    Name,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Spot {
    dimension: Dimension,
    x: i32,
    z: i32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Target {
    dimension: Dimension,
    x: i32,
    y: i32,
    z: i32,
}

struct NameForm {
    text: String,
    target: Option<Target>,
    rename: Option<Spot>,
    background: Mode,
}

#[derive(Clone, Copy)]
struct Menu {
    height_approximate: bool,
    block: [i32; 2],
    height: Option<i32>,
    dimension: Dimension,
    at: [f32; 2],
    portal: Option<Portal>,
    waypoint: Option<Spot>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuAction {
    Copy,
    Waypoint,
    CopyDestination,
    Teleport,
    TeleportDestination,
    DeleteWaypoint,
    Information,
}

struct State {
    world: String,
    session: u64,
    dimension: Dimension,
    seen: BTreeMap<(i32, i32), (Weak<Chunk>, Layer, Instant)>,
    layers: BTreeMap<Dimension, Layer>,
    selected: Dimension,
    large: bool,
    camera: maps::Camera,
    marker: [f64; 2],
    elapsed: f64,
    uploaded: (u64, usize),
    frame: Option<Arc<maps::Frame>>,
    dragging: bool,
    close: bool,
    mode: Mode,
    menu: Option<Menu>,
    name_form: Option<NameForm>,
    waypoints: Vec<Waypoint>,
    portals: Vec<(Dimension, [i32; 3], Portal)>,
    waypoint_scroll: usize,
    waypoint_selected: Option<usize>,
    notice: Option<(String, Instant)>,
    pending_copy: Option<String>,
    pending_command: Option<String>,
    map_focus: Option<Spot>,
    waypoint_biomes: BTreeMap<Spot, String>,
    lod_elapsed: f64,
    lod_sources:
        BTreeMap<(Dimension, maps::LodKey), (Weak<Vec<horizon_client::lod::Column>>, Instant)>,
    info: Option<Menu>,
    info_scroll: usize,
}
impl Default for State {
    fn default() -> Self {
        Self {
            world: String::new(),
            session: 0,
            dimension: Dimension::Player,
            seen: BTreeMap::new(),
            layers: BTreeMap::new(),
            selected: Dimension::Player,
            large: false,
            camera: maps::Camera::new([0.0; 2], 2.0),
            marker: [0.0; 2],
            elapsed: 1.0,
            uploaded: (0, 0),
            frame: None,
            dragging: false,
            close: false,
            mode: Mode::Map,
            menu: None,
            name_form: None,
            waypoints: Vec::new(),
            portals: Vec::new(),
            waypoint_scroll: 0,
            waypoint_selected: None,
            notice: None,
            pending_copy: None,
            pending_command: None,
            map_focus: None,
            waypoint_biomes: BTreeMap::new(),
            lod_elapsed: 1.0,
            lod_sources: BTreeMap::new(),
            info: None,
            info_scroll: 0,
        }
    }
}
static STATE: OnceLock<Mutex<State>> = OnceLock::new();
fn state() -> std::sync::MutexGuard<'static, State> {
    STATE
        .get_or_init(|| Mutex::new(State::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}
fn rectangle(width: f32, height: f32) -> [f32; 4] {
    [0.0, 0.0, width.max(1.0), height.max(1.0)]
}
fn inside(mouse: [f32; 2], rect: [f32; 4]) -> bool {
    mouse[0] >= rect[0]
        && mouse[1] >= rect[1]
        && mouse[0] < rect[0] + rect[2]
        && mouse[1] < rect[1] + rect[3]
}

// Ground cover the map leaves out: grass, ferns, bushes and dry grass.
fn hidden_cover(classes: &[Arc<str>], name: &str) -> bool {
    classes.iter().any(|class| {
        matches!(
            class.as_ref(),
            "TallGrassBlock" | "DryVegetationBlock" | "BushBlock" | "FireflyBushBlock"
        )
    }) || matches!(
        name,
        "minecraft:tall_grass" | "minecraft:large_fern" | "minecraft:grass"
    )
}

// Curated petal, crop and plant colours: the vanilla map colour of most plants
// is one generic green, which hides what actually grows there.
fn plant_color(name: &str) -> Option<u32> {
    let short = name.strip_prefix("minecraft:").unwrap_or(name);
    if let Some(inner) = short.strip_prefix("potted_")
        && let Some(color) = plant_color(&format!("minecraft:{inner}"))
    {
        return Some(color);
    }
    Some(match short {
        "poppy" => 0xc0392b,
        "dandelion" => 0xf1c40f,
        "blue_orchid" => 0x2e86c1,
        "allium" => 0xb57edc,
        "azure_bluet" => 0xf0f0f0,
        "oxeye_daisy" => 0xf5f5dc,
        "cornflower" => 0x4a69bd,
        "lily_of_the_valley" => 0xf8f8f8,
        "wither_rose" => 0x2c2c2c,
        "orange_tulip" => 0xe67e22,
        "red_tulip" => 0xc0392b,
        "white_tulip" => 0xecf0f1,
        "pink_tulip" => 0xe88bb0,
        "sunflower" => 0xf4d03f,
        "lilac" => 0xc39bd3,
        "rose_bush" => 0xc0392b,
        "peony" => 0xf5b7d7,
        "torchflower" => 0xe67e22,
        "cactus_flower" => 0xe88bb0,
        "open_eyeblossom" => 0xee8a26,
        "closed_eyeblossom" => 0x7f8c8d,
        "golden_dandelion" => 0xd4a017,
        "wildflowers" | "pink_petals" => 0xf08ac0,
        "spore_blossom" => 0xe88bb0,
        "chorus_flower" => 0xa569bd,
        "wheat" => 0xd9b44a,
        "carrots" => 0xe67e22,
        "potatoes" => 0xb8a06a,
        "beetroots" => 0xa03040,
        "nether_wart" => 0xa03040,
        "melon_stem" | "attached_melon_stem" | "pumpkin_stem" | "attached_pumpkin_stem" => 0x5b8a3c,
        "pitcher_crop" => 0x9dc183,
        "torchflower_crop" => 0xe67e22,
        "oak_sapling" | "spruce_sapling" | "birch_sapling" | "jungle_sapling"
        | "acacia_sapling" | "dark_oak_sapling" | "cherry_sapling" | "pale_oak_sapling"
        | "poplar_sapling" => 0x4f7f2f,
        "mangrove_propagule" | "bamboo_sapling" => 0x6a9f3f,
        "red_mushroom" => 0xd94b44,
        "brown_mushroom" => 0x9a755c,
        "crimson_fungus" => 0xa03030,
        "warped_fungus" => 0x4a8f7f,
        "crimson_roots" => 0x8a3030,
        "warped_roots" => 0x3f7f7f,
        "nether_sprouts" => 0x3f8f5f,
        "vine" | "cave_vines" | "cave_vines_plant" => 0x3f6f2f,
        "twisting_vines" | "twisting_vines_plant" => 0x3fa0a0,
        "weeping_vines" | "weeping_vines_plant" => 0x8a3030,
        "sugar_cane" => 0x8fbf5a,
        "bamboo" => 0xb7c65a,
        "cactus" => 0x3f7f3f,
        "lily_pad" => 0x3f7f3f,
        "sea_pickle" => 0x6a8f3f,
        "seagrass" | "tall_seagrass" | "kelp" | "kelp_plant" => 0x3f9f3f,
        "moss_carpet" | "pale_moss_carpet" | "moss_block" => 0x5f8f3f,
        "fern" | "short_grass" | "tall_grass" | "large_fern" => 0x4f8f3f,
        "dead_bush" | "bush" | "red_shrub" | "firefly_bush" => 0x8a6a3a,
        "leaf_litter" => 0x8a6a3a,
        "pale_hanging_moss" => 0x8a9f8a,
        "hanging_roots" => 0x8a6a4a,
        "mangrove_roots" => 0x6a5a3a,
        "big_dripleaf" | "big_dripleaf_stem" | "small_dripleaf" => 0x4f8f3f,
        "azalea" | "flowering_azalea" | "azalea_bush" | "flowering_azalea_bush" => 0x4f8f3f,
        "sweet_berry_bush" => 0x5f8f3f,
        "nether_wart_block" => 0xa03040,
        "warped_wart_block" => 0x2f8f8f,
        _ => return None,
    })
}

// The dye of a stained pane, whose vanilla map colour is unset.
fn dye_color(name: &str) -> Option<u32> {
    let short = name.strip_prefix("minecraft:").unwrap_or(name);
    let dye = short.strip_suffix("_stained_glass_pane")?;
    Some(match dye {
        "white" => 0xecf0f1,
        "orange" => 0xe67e22,
        "magenta" => 0xc0398b,
        "light_blue" => 0x5dade2,
        "yellow" => 0xf4d03f,
        "lime" => 0x7dce3a,
        "pink" => 0xf5b7d7,
        "gray" => 0x7f8c8d,
        "light_gray" => 0xbdc3c7,
        "cyan" => 0x2e9a9a,
        "purple" => 0x8e44ad,
        "blue" => 0x2e4fa3,
        "brown" => 0x8a5a2b,
        "green" => 0x2e7d32,
        "red" => 0xc0392b,
        "black" => 0x1c1c1c,
        _ => return None,
    })
}

fn portal_tint(kind: Portal) -> (u32, u8) {
    match kind {
        Portal::Nether => (0x9b4dff, 150),
        Portal::End => (0x4a1a6a, 190),
        Portal::Gateway => (0x1a0a2a, 190),
    }
}

fn draw_icon(
    game: &Game<'_>,
    batch: &mut HudBatch,
    at: [f32; 2],
    rows: [&str; 8],
    colors: [u32; 3],
) {
    for (y, row) in rows.iter().enumerate() {
        let bytes = row.as_bytes();
        let mut x = 0;
        while x < bytes.len() {
            let color = bytes[x];
            let start = x;
            while x < bytes.len() && bytes[x] == color {
                x += 1;
            }
            if let Some(index) = color.checked_sub(b'1').filter(|i| *i < 3) {
                game.native_fill(
                    batch,
                    [
                        at[0] + start as f32,
                        at[1] + y as f32,
                        (x - start) as f32,
                        1.0,
                    ],
                    colors[index as usize],
                );
            }
        }
    }
}

fn dimension_icon(game: &Game<'_>, batch: &mut HudBatch, dimension: Dimension, at: [f32; 2]) {
    let (rows, colors) = match dimension {
        Dimension::Player => (
            [
                "...11...", "..1111..", ".111111.", "...11...", "...11...", "...11...", "...11...",
                "........",
            ],
            [0xffff6262, 0xffffffff, 0xffffffff],
        ),
        Dimension::Overworld => (
            [
                "........", "........", "11111111", "11111111", "22222222", "22322232", "23222322",
                "22232222",
            ],
            [0xff68b34a, 0xff93643d, 0xffbc8a54],
        ),
        Dimension::Nether => (
            [
                ".111111.", ".122221.", ".123321.", ".123321.", ".123321.", ".122221.", ".111111.",
                "........",
            ],
            [0xff44263f, 0xffb649cf, 0xffef9aff],
        ),
        Dimension::End => (
            [
                "...11...", ".112211.", "12233221", "12333321", ".123321.", "..1221..", "...11...",
                "........",
            ],
            [0xffc6c78c, 0xff598f88, 0xff191d35],
        ),
    };
    draw_icon(game, batch, at, rows, colors);
}

fn known_biome(game: &Game<'_>, dimension: Dimension, at: [i32; 3]) -> Option<String> {
    let player = game.native_player()?;
    if player.dimension != dimension.id()
        || !game
            .world
            .chunks
            .contains_key(&(at[0].div_euclid(16), at[2].div_euclid(16)))
    {
        return None;
    }
    game.native_biome(at)
}

fn localized_biome(id: &str) -> String {
    horizon_client::lang::get_or(&format!("biome.{}", id.replace(':', ".")), id)
}

fn biome_icon(game: &Game<'_>, batch: &mut HudBatch, biome: Option<&str>, at: [f32; 2]) {
    use runtime::BiomeSymbol::*;
    let (rows, colors) = match runtime::biome_symbol(biome) {
        Unknown => (
            [
                "..1111..", ".11..11.", ".....11.", "...111..", "...11...", "........", "...11...",
                "........",
            ],
            [0xffaeb9c6, 0, 0],
        ),
        Water => (
            [
                "........", "1..11..1", ".11..11.", "........", "1..11..1", ".11..11.", "........",
                "........",
            ],
            [0xff68b8ed, 0, 0],
        ),
        Desert => (
            [
                "..1.....", ".111....", "..1.....", ".....2..", "...2.22.", "...222..", ".....2..",
                "33333333",
            ],
            [0xffffd65c, 0xff6b9952, 0xffd9b977],
        ),
        Snow => (
            [
                "...1....", ".1.1.1..", "..111...", "1111111.", "..111...", ".1.1.1..", "...1....",
                "........",
            ],
            [0xffd2f0ff, 0, 0],
        ),
        Forest => (
            [
                "...1....", "..111...", ".11111..", "..111...", ".11111..", "1111111.", "...2....",
                "...2....",
            ],
            [0xff4caa65, 0xff9b7045, 0],
        ),
        Swamp => (
            [
                "...1....", "..111...", ".11111..", "1.121.1.", "..121...", "...2....", "33333333",
                ".33..33.",
            ],
            [0xff75964b, 0xff82664c, 0xff568c85],
        ),
        Mushroom => (
            [
                "..1111..", ".111111.", "11211211", "11111111", "...22...", "...22...", "...22...",
                "........",
            ],
            [0xffce6262, 0xffeee3ca, 0],
        ),
        Badlands => (
            [
                ".....11.", "....1111", "..111111", ".2222222", "22222222", "33333333", "11111111",
                "22222222",
            ],
            [0xffc9824a, 0xffb9553d, 0xffecd39b],
        ),
        Nether => (
            [
                "....1...", "...11...", ".1.112..", ".111122.", "11112221", "11122211", ".112211.",
                "..1111..",
            ],
            [0xffd64e44, 0xffffb552, 0],
        ),
        End => (
            [
                "........", "...11...", "..1221..", ".122221.", "12233221", ".122221.", "..1111..",
                "........",
            ],
            [0xffc8c78c, 0xffdddca8, 0xffa586c7],
        ),
        Cave => (
            [
                "..1111..", ".112211.", "11222211", "12222221", "12222221", "12222221", "11222211",
                "11111111",
            ],
            [0xff9b8b85, 0xff333141, 0],
        ),
        Plains => (
            [
                "........", "...1....", "..121...", "...1....", "...3....", ".3.3....", "..33....",
                "33333333",
            ],
            [0xfff0c763, 0xfffff3d0, 0xff70b65a],
        ),
    };
    draw_icon(game, batch, at, rows, colors);
}

fn portal_label(kind: Portal) -> &'static str {
    match kind {
        Portal::Nether => tr(
            "Nether portal",
            "Портал Незера",
            "Portail du Nether",
            "Portal del Nether",
            "Netherportal",
        ),
        Portal::End => tr(
            "End portal",
            "Портал Энда",
            "Portail de l’End",
            "Portal del End",
            "Endportal",
        ),
        Portal::Gateway => tr(
            "End gateway",
            "Врата Энда",
            "Passerelle de l’End",
            "Puerta de enlace del End",
            "Endtransitportal",
        ),
    }
}

// The world block under the pointer, when the pointer is over the map.
fn hovered_block(view: &View, rect: [f32; 4], mouse: [f32; 2]) -> Option<[i32; 2]> {
    if !inside(mouse, rect) {
        return None;
    }
    let x = f64::from((mouse[0] - rect[0]) / rect[2]) * view.width as f64;
    let y = f64::from((mouse[1] - rect[1]) / rect[3]) * view.height as f64;
    Some(view.block_at(x, y))
}

// A block's rectangle in GUI pixels, from the frame's own pixel mapping.
fn block_rect(view: &View, rect: [f32; 4], block: [i32; 2]) -> [f32; 4] {
    let first = view.screen_at([f64::from(block[0]), f64::from(block[1])]);
    let second = view.screen_at([f64::from(block[0]) + 1.0, f64::from(block[1]) + 1.0]);
    let scale_x = rect[2] / view.width as f32;
    let scale_y = rect[3] / view.height as f32;
    [
        rect[0] + first[0].min(second[0]) as f32 * scale_x,
        rect[1] + first[1].min(second[1]) as f32 * scale_y,
        (first[0] - second[0]).abs() as f32 * scale_x,
        (first[1] - second[1]).abs() as f32 * scale_y,
    ]
}

fn highlight(game: &Game<'_>, batch: &mut HudBatch, view: &View, rect: [f32; 4], block: [i32; 2]) {
    let cell = block_rect(view, rect, block);
    if cell[2] < 1.0 || cell[3] < 1.0 {
        return;
    }
    let left = cell[0].max(rect[0]);
    let top = cell[1].max(rect[1]);
    let right = (cell[0] + cell[2]).min(rect[0] + rect[2]);
    let bottom = (cell[1] + cell[3]).min(rect[1] + rect[3]);
    let (width, height) = (right - left, bottom - top);
    if width < 1.0 || height < 1.0 {
        return;
    }
    game.native_fill(batch, [left, top, width, height], 0x40ffffff);
    let line = 1.0_f32.min(width).min(height);
    game.native_fill(batch, [left, top, width, line], 0xff000000);
    game.native_fill(batch, [left, top + height - line, width, line], 0xff000000);
    game.native_fill(batch, [left, top, line, height], 0xff000000);
    game.native_fill(batch, [left + width - line, top, line, height], 0xff000000);
}

fn draw_marker(
    game: &Game<'_>,
    batch: &mut HudBatch,
    view: &View,
    rect: [f32; 4],
    at: [f64; 2],
    player: &crate::game::native_hooks::NativePlayer,
) {
    let pixel = view.screen_at(at);
    let x = rect[0] + pixel[0] as f32 * rect[2] / view.width as f32;
    let y = rect[1] + pixel[1] as f32 * rect[3] / view.height as f32;
    if !inside([x, y], rect) {
        return;
    }
    let (border, fill) = if Dimension::from_id(player.dimension) == view.dimension {
        (0xfff0f4fa, 0xffff5050)
    } else {
        (0xc0f0f4fa, 0xc060a0ff)
    };
    let forward = view.player_heading(f64::from(player.yaw), [rect[2], rect[3]]);
    let right = [-forward[1], forward[0]];
    for (size, color) in [(7.0, border), (5.0, fill)] {
        let point = |along: f32, across: f32| {
            [
                x + size * (forward[0] * along + right[0] * across),
                y + size * (forward[1] * along + right[1] * across),
            ]
        };
        let tip = point(1.0, 0.0);
        game.native_quad(
            batch,
            [tip, point(-0.65, 0.55), point(-0.65, -0.55), tip],
            color,
        );
    }
}

fn first_letter(name: &str) -> char {
    name.chars()
        .find(|c| !c.is_whitespace())
        .unwrap_or('?')
        .to_uppercase()
        .next()
        .unwrap_or('?')
}

fn draw_waypoint(
    game: &Game<'_>,
    batch: &mut HudBatch,
    view: &View,
    rect: [f32; 4],
    waypoint: &Waypoint,
    size: f32,
) {
    let pixel = view.screen_at([f64::from(waypoint.x) + 0.5, f64::from(waypoint.z) + 0.5]);
    let x = rect[0] + pixel[0] as f32 * rect[2] / view.width as f32;
    let y = rect[1] + pixel[1] as f32 * rect[3] / view.height as f32;
    if !inside([x, y], rect) {
        return;
    }
    let half = size / 2.0;
    game.native_fill(
        batch,
        [x - half - 1.0, y - half - 1.0, size + 2.0, size + 2.0],
        0xff101010,
    );
    game.native_fill(batch, [x - half, y - half, size, size], 0xffffd24a);
    let letter = first_letter(&waypoint.name).to_string();
    game.native_text(
        batch,
        &letter,
        [x - game.native_text_width(&letter) / 2.0, y - 4.0],
        0xff201800,
        false,
    );
}

fn draw_portal_marker(
    game: &Game<'_>,
    batch: &mut HudBatch,
    view: &View,
    rect: [f32; 4],
    at: [i32; 3],
    kind: Portal,
) {
    let pixel = view.screen_at([f64::from(at[0]) + 0.5, f64::from(at[2]) + 0.5]);
    let x = rect[0] + pixel[0] as f32 * rect[2] / view.width as f32;
    let y = rect[1] + pixel[1] as f32 * rect[3] / view.height as f32;
    if !inside([x, y], rect) {
        return;
    }
    let (color, _) = portal_tint(kind);
    game.native_fill(batch, [x - 3.5, y - 3.5, 7.0, 7.0], 0xff101010);
    game.native_fill(batch, [x - 2.5, y - 2.5, 5.0, 5.0], color);
    match kind {
        Portal::Nether => {
            game.native_fill(batch, [x - 0.5, y - 2.0, 1.0, 4.0], 0xfff0e0ff);
            game.native_fill(batch, [x - 2.0, y - 0.5, 4.0, 1.0], 0xfff0e0ff);
        }
        Portal::End => {
            game.native_fill(batch, [x - 1.0, y - 1.0, 2.0, 2.0], 0xffe0d0ff);
        }
        Portal::Gateway => {
            game.native_fill(batch, [x - 1.5, y - 1.5, 3.0, 3.0], 0xffb090ff);
        }
    }
}

// The visible height at a hovered column, in the player's current dimension
// (the only one whose loaded chunks the client holds).
fn hovered_height(game: &Game<'_>, layer: Layer, block: [i32; 2]) -> Option<i32> {
    let chunk = game
        .world
        .chunks
        .get(&(block[0].div_euclid(16), block[1].div_euclid(16)))?;
    let (x, z) = (block[0].rem_euclid(16), block[1].rem_euclid(16));
    let facts = game.res.facts();
    let ceiling = match layer {
        Layer::Surface => i32::MAX,
        Layer::Below(y) => y,
    };
    chunk.highest(game.world.air.0, |y| {
        if y > ceiling {
            return false;
        }
        let id = chunk.block(x, y, z);
        let Some(entry) = facts.states.get(id.0 as usize) else {
            return false;
        };
        if hidden_cover(entry.classes.as_ref(), entry.name.as_ref()) {
            return false;
        }
        !facts.is_air(id)
            && horizon_client::blocks::MAP_COLORS
                .get(entry.map_color as usize)
                .copied()
                .unwrap_or(0)
                != 0
    })
}

const MENU_HEADER: f32 = 19.0;
const MENU_SUBLINE: f32 = 14.0;
const MENU_ROW: f32 = 15.0;
const MENU_PAD: f32 = 4.0;

fn teleport_allowed(game: &Game<'_>) -> bool {
    game.ui.registries.permission >= 2
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PortalTarget {
    Exact { dimension: Dimension, at: [i32; 3] },
    Spawn,
    Outer,
    Unknown,
}

fn portal_target(state: &State, menu: Menu, player_y: i32) -> Option<PortalTarget> {
    let kind = menu.portal?;
    Some(match kind {
        Portal::Nether => {
            let (dimension, nominal) = match menu.dimension {
                Dimension::Overworld => (
                    Dimension::Nether,
                    [
                        menu.block[0] / 8,
                        menu.height.unwrap_or(player_y),
                        menu.block[1] / 8,
                    ],
                ),
                Dimension::Nether => (
                    Dimension::Overworld,
                    [
                        menu.block[0] * 8,
                        menu.height.unwrap_or(player_y),
                        menu.block[1] * 8,
                    ],
                ),
                _ => return Some(PortalTarget::Unknown),
            };
            let nearest = state
                .portals
                .iter()
                .filter(|(at_dimension, _, at_kind)| {
                    *at_dimension == dimension && *at_kind == Portal::Nether
                })
                .map(|(_, at, _)| *at)
                .filter(|at| {
                    let dx = f64::from(at[0] - nominal[0]);
                    let dz = f64::from(at[2] - nominal[2]);
                    dx * dx + dz * dz <= 128.0 * 128.0
                })
                .min_by_key(|at| {
                    let dx = i64::from(at[0] - nominal[0]);
                    let dz = i64::from(at[2] - nominal[2]);
                    dx * dx + dz * dz
                });
            PortalTarget::Exact {
                dimension,
                at: nearest.unwrap_or(nominal),
            }
        }
        Portal::End => {
            if menu.dimension == Dimension::End {
                PortalTarget::Spawn
            } else {
                PortalTarget::Exact {
                    dimension: Dimension::End,
                    at: [100, 49, 0],
                }
            }
        }
        Portal::Gateway => PortalTarget::Outer,
    })
}

fn portal_target_text(target: PortalTarget) -> String {
    match target {
        PortalTarget::Exact { dimension, at } => {
            format!("{}: {} {} {}", dimension.label(), at[0], at[1], at[2])
        }
        PortalTarget::Spawn => tr(
            "Overworld: spawn point",
            "Верхний мир: точка спавна",
            "Surface : point d’apparition",
            "Mundo normal: punto de aparición",
            "Oberwelt: Spawnpunkt",
        )
        .into(),
        PortalTarget::Outer => tr(
            "End: outer islands",
            "Энд: внешние острова",
            "End : îles extérieures",
            "End: islas exteriores",
            "Ende: äußere Inseln",
        )
        .into(),
        PortalTarget::Unknown => tr(
            "unknown destination",
            "назначение неизвестно",
            "destination inconnue",
            "destino desconocido",
            "Unbekanntes Ziel",
        )
        .into(),
    }
}

fn menu_title(menu: Menu) -> String {
    match menu.height {
        Some(y) => format!(
            "X: {}  Y: {}{}  Z: {}",
            menu.block[0],
            if menu.height_approximate { "≈" } else { "" },
            y,
            menu.block[1]
        ),
        None => format!("X: {}  Z: {}", menu.block[0], menu.block[1]),
    }
}

fn menu_subtitle(
    state: &State,
    menu: Menu,
    player: &crate::game::native_hooks::NativePlayer,
) -> Option<String> {
    menu.portal.map(|kind| {
        let target = portal_target(state, menu, player.y.floor() as i32);
        format!(
            "{} → {}",
            portal_label(kind),
            target.map_or_else(
                || tr(
                    "unknown destination",
                    "назначение неизвестно",
                    "destination inconnue",
                    "destino desconocido",
                    "Unbekanntes Ziel"
                )
                .to_owned(),
                portal_target_text
            )
        )
    })
}

fn menu_items(
    game: &Game<'_>,
    menu: Menu,
    target: Option<PortalTarget>,
) -> Vec<(&'static str, MenuAction)> {
    let mut items = vec![
        (
            tr(
                "Chunk and region information",
                "Информация о чанке и регионе",
                "Informations sur le chunk et la région",
                "Información del chunk y la región",
                "Chunk- und Regionsinformationen",
            ),
            MenuAction::Information,
        ),
        (
            tr(
                "Copy coordinates",
                "Скопировать координаты",
                "Copier les coordonnées",
                "Copiar coordenadas",
                "Koordinaten kopieren",
            ),
            MenuAction::Copy,
        ),
        (
            tr(
                "Create waypoint",
                "Поставить вейпоинт",
                "Créer un repère",
                "Crear marcador",
                "Wegpunkt erstellen",
            ),
            MenuAction::Waypoint,
        ),
    ];
    if menu.portal.is_some() && matches!(target, Some(PortalTarget::Exact { .. })) {
        items.push((
            tr(
                "Copy destination coordinates",
                "Скопировать координаты назначения",
                "Copier les coordonnées de destination",
                "Copiar coordenadas del destino",
                "Zielkoordinaten kopieren",
            ),
            MenuAction::CopyDestination,
        ));
    }
    if menu.waypoint.is_some() {
        items.push((
            tr(
                "Delete waypoint",
                "Удалить вейпоинт",
                "Supprimer le repère",
                "Eliminar marcador",
                "Wegpunkt löschen",
            ),
            MenuAction::DeleteWaypoint,
        ));
    }
    if teleport_allowed(game) {
        if menu.portal.is_some() {
            if matches!(target, Some(PortalTarget::Exact { .. })) {
                items.push((
                    tr(
                        "Teleport to portal",
                        "Телепортироваться к порталу",
                        "Se téléporter au portail",
                        "Teletransportarse al portal",
                        "Zum Portal teleportieren",
                    ),
                    MenuAction::TeleportDestination,
                ));
            }
        } else {
            items.push((
                tr(
                    "Teleport here",
                    "Телепортироваться туда",
                    "Se téléporter ici",
                    "Teletransportarse aquí",
                    "Hierher teleportieren",
                ),
                MenuAction::Teleport,
            ));
        }
    }
    items
}

fn menu_layout(
    game: &Game<'_>,
    size: [f32; 2],
    menu: Menu,
    subtitle: Option<&str>,
    items: &[(&'static str, MenuAction)],
) -> ([f32; 4], Vec<[f32; 4]>) {
    let mut width = game.native_text_width(&menu_title(menu));
    if let Some(subtitle) = subtitle {
        width = width.max(game.native_text_width(subtitle));
    }
    for (label, _) in items {
        width = width.max(game.native_text_width(label));
    }
    let width = (width + 18.0).clamp(160.0, 340.0);
    let header = MENU_HEADER
        + if subtitle.is_some() {
            MENU_SUBLINE
        } else {
            0.0
        };
    let height = header + items.len() as f32 * MENU_ROW + MENU_PAD;
    let x = if menu.at[0] + width > size[0] - 6.0 {
        size[0] - width - 6.0
    } else {
        menu.at[0]
    }
    .max(6.0);
    let y = if menu.at[1] + height > size[1] - 6.0 {
        size[1] - height - 6.0
    } else {
        menu.at[1]
    }
    .max(6.0);
    let rect = [x, y, width, height];
    let rows = (0..items.len())
        .map(|index| {
            [
                rect[0] + 4.0,
                rect[1] + header + index as f32 * MENU_ROW,
                rect[2] - 8.0,
                MENU_ROW,
            ]
        })
        .collect();
    (rect, rows)
}

fn teleport_command(dimension: Dimension, at: [i32; 3], from: Dimension) -> String {
    let coords = format!(
        "{:.3} {:.3} {:.3}",
        f64::from(at[0]) + 0.5,
        f64::from(at[1]),
        f64::from(at[2]) + 0.5
    );
    if dimension == from {
        format!("tp {coords}")
    } else {
        format!("execute in {} run tp {coords}", dimension.id())
    }
}

fn default_waypoint_name(state: &State) -> String {
    localized_format!(
        "Waypoint {}",
        "Вейпоинт {}",
        "Repère {}",
        "Marcador {}",
        "Wegpunkt {}",
        state.waypoints.len() + 1
    )
}

fn perform_menu(game: &Game<'_>, state: &mut State, menu: Menu, action: MenuAction) {
    let Some(player) = game.native_player() else {
        state.menu = None;
        return;
    };
    let fallback = player.y.floor() as i32;
    let y = menu.height.unwrap_or(fallback);
    let target = portal_target(state, menu, fallback);
    state.menu = None;
    match action {
        MenuAction::Information => {
            state.info = Some(menu);
            state.info_scroll = 0;
        }
        MenuAction::Copy => {
            state.pending_copy = Some(format!("{} {} {}", menu.block[0], y, menu.block[1]));
            state.notice = Some((
                tr(
                    "Coordinates copied",
                    "Координаты скопированы",
                    "Coordonnées copiées",
                    "Coordenadas copiadas",
                    "Koordinaten kopiert",
                )
                .into(),
                Instant::now(),
            ));
        }
        MenuAction::Waypoint => {
            let target = Target {
                dimension: menu.dimension,
                x: menu.block[0],
                y,
                z: menu.block[1],
            };
            state.name_form = Some(NameForm {
                text: default_waypoint_name(state),
                target: Some(target),
                rename: None,
                background: Mode::Map,
            });
            state.mode = Mode::Name;
            runtime::set_text_input(true);
        }
        MenuAction::CopyDestination => {
            if let Some(PortalTarget::Exact { at, .. }) = target {
                state.pending_copy = Some(format!("{} {} {}", at[0], at[1], at[2]));
                state.notice = Some((
                    tr(
                        "Destination coordinates copied",
                        "Координаты назначения скопированы",
                        "Coordonnées de destination copiées",
                        "Coordenadas del destino copiadas",
                        "Zielkoordinaten kopiert",
                    )
                    .into(),
                    Instant::now(),
                ));
            }
        }
        MenuAction::Teleport => {
            if !teleport_allowed(game) {
                return;
            }
            state.pending_command = Some(teleport_command(
                menu.dimension,
                [menu.block[0], y + 1, menu.block[1]],
                Dimension::from_id(player.dimension),
            ));
            state.close = true;
        }
        MenuAction::TeleportDestination => {
            if !teleport_allowed(game) {
                return;
            }
            if let Some(PortalTarget::Exact { dimension, at }) = target {
                state.pending_command = Some(teleport_command(
                    dimension,
                    at,
                    Dimension::from_id(player.dimension),
                ));
                state.close = true;
            }
        }
        MenuAction::DeleteWaypoint => {
            if let Some(spot) = menu.waypoint {
                runtime::initialize(game.native_data_dir(runtime::MOD_ID)).remove_waypoint(
                    &state.world,
                    spot.dimension,
                    spot.x,
                    spot.z,
                );
                state.notice = Some((
                    tr(
                        "Waypoint deleted",
                        "Вейпоинт удалён",
                        "Repère supprimé",
                        "Marcador eliminado",
                        "Wegpunkt gelöscht",
                    )
                    .into(),
                    Instant::now(),
                ));
            }
        }
    }
}

fn draw_menu(game: &Game<'_>, batch: &mut HudBatch, state: &State) {
    let Some(menu) = state.menu else {
        return;
    };
    let Some(player) = game.native_player() else {
        return;
    };
    let subtitle = menu_subtitle(state, menu, &player);
    let has_subtitle = subtitle.is_some();
    let target = portal_target(state, menu, player.y.floor() as i32);
    let items = menu_items(game, menu, target);
    let (rect, rows) = menu_layout(
        game,
        [batch.width, batch.height],
        menu,
        subtitle.as_deref(),
        &items,
    );
    let mouse = game.native_cursor();
    let border = 0xff3a4553;
    game.native_fill(batch, rect, 0xf01b2028);
    game.native_fill(batch, [rect[0], rect[1], rect[2], 1.0], border);
    game.native_fill(
        batch,
        [rect[0], rect[1] + rect[3] - 1.0, rect[2], 1.0],
        border,
    );
    game.native_fill(batch, [rect[0], rect[1], 1.0, rect[3]], border);
    game.native_fill(
        batch,
        [rect[0] + rect[2] - 1.0, rect[1], 1.0, rect[3]],
        border,
    );
    game.native_text(
        batch,
        &menu_title(menu),
        [rect[0] + 8.0, rect[1] + 5.0],
        0xffffff,
        true,
    );
    if let Some(subtitle) = subtitle.as_deref() {
        game.native_text(
            batch,
            subtitle,
            [rect[0] + 8.0, rect[1] + MENU_HEADER - 3.0],
            0xd8b0ff,
            false,
        );
    }
    let header = MENU_HEADER + if has_subtitle { MENU_SUBLINE } else { 0.0 };
    game.native_fill(
        batch,
        [rect[0] + 4.0, rect[1] + header - 1.0, rect[2] - 8.0, 1.0],
        border,
    );
    for ((label, _), row) in items.iter().zip(rows) {
        if inside(mouse, row) {
            game.native_fill(batch, row, 0xff2e3a48);
        }
        game.native_text(batch, label, [row[0] + 7.0, row[1] + 4.0], 0xdddddd, false);
    }
}

fn draw_notice(game: &Game<'_>, batch: &mut HudBatch, state: &State) {
    let Some((text, since)) = state.notice.as_ref() else {
        return;
    };
    if since.elapsed() > Duration::from_secs(2) {
        return;
    }
    let width = game.native_text_width(text);
    let x = (batch.width - width) / 2.0;
    let y = batch.height - 38.0;
    game.native_fill(batch, [x - 7.0, y - 4.0, width + 14.0, 14.0], 0xd01b2028);
    game.native_text(batch, text, [x, y], 0xffffd24a, true);
}

fn button(
    game: &Game<'_>,
    batch: &mut HudBatch,
    rect: [f32; 4],
    label: &str,
    mouse: [f32; 2],
    enabled: bool,
) {
    let color = if !enabled {
        0xff232a33
    } else if inside(mouse, rect) {
        0xff3a4a5c
    } else {
        0xff2a3542
    };
    game.native_fill(batch, rect, color);
    let text_color = if enabled { 0xffffff } else { 0x707880 };
    game.native_text(
        batch,
        label,
        [
            rect[0] + (rect[2] - game.native_text_width(label)) / 2.0,
            rect[1] + 4.0,
        ],
        text_color,
        false,
    );
}

fn waypoint_visible_rows(height: f32) -> usize {
    (((height - 90.0) / 28.0).floor() as usize).clamp(1, 12)
}

fn waypoint_row_at(state: &State, width: f32, height: f32, index: usize) -> Option<[f32; 4]> {
    let start = state.waypoint_scroll;
    if index < start || index >= start + waypoint_visible_rows(height) {
        return None;
    }
    let row = index - start;
    Some([
        12.0,
        44.0 + row as f32 * 28.0,
        (width - 24.0).max(1.0),
        27.0,
    ])
}

// Shared by drawing and hit testing, including narrow GUI viewports.
fn waypoint_actions(width: f32, height: f32) -> [[f32; 4]; 3] {
    let button_width = ((width - 36.0) / 3.0).max(1.0);
    std::array::from_fn(|i| {
        [
            12.0 + i as f32 * (button_width + 6.0),
            height - 42.0,
            button_width,
            18.0,
        ]
    })
}

fn show_selected_waypoint(state: &mut State) -> Option<Dimension> {
    let waypoint = state.waypoints.get(state.waypoint_selected?)?;
    if waypoint.world != state.world {
        return None;
    }
    let spot = Spot {
        dimension: waypoint.dimension,
        x: waypoint.x,
        z: waypoint.z,
    };
    state.map_focus = Some(spot);
    state.mode = Mode::Map;
    state.menu = None;
    state.dragging = false;
    state.elapsed = 1.0;
    Some(spot.dimension)
}

// Apply after the normal dimension/mode transition so it cannot recenter on
// the player and overwrite the requested waypoint on the following frame.
fn apply_map_focus(state: &mut State, large: bool, selected: Dimension) {
    if large
        && let Some(spot) = state.map_focus.take()
        && spot.dimension == selected
    {
        state
            .camera
            .snap([f64::from(spot.x) + 0.5, f64::from(spot.z) + 0.5]);
        state.frame = None;
    }
}

fn draw_waypoints_screen(game: &Game<'_>, batch: &mut HudBatch, state: &State) {
    game.native_fill(batch, [0.0, 0.0, batch.width, batch.height], 0xa611151b);
    game.native_text(
        batch,
        tr(
            "Waypoints",
            "Вейпоинты",
            "Repères",
            "Marcadores",
            "Wegpunkte",
        ),
        [12.0, 8.0],
        0xffffff,
        true,
    );
    let mouse = game.native_cursor();
    let create = [batch.width - 260.0, 6.0, 120.0, 18.0];
    let back = [batch.width - 132.0, 6.0, 120.0, 18.0];
    button(
        game,
        batch,
        create,
        tr(
            "Create here",
            "Создать здесь",
            "Créer ici",
            "Crear aquí",
            "Hier erstellen",
        ),
        mouse,
        true,
    );
    button(
        game,
        batch,
        back,
        tr("Back", "Назад", "Retour", "Volver", "Zurück"),
        mouse,
        true,
    );
    game.native_fill(batch, [12.0, 28.0, batch.width - 24.0, 1.0], 0xff3a4553);
    if state.waypoints.is_empty() {
        game.native_text(
            batch,
            tr(
                "No waypoints yet. Right-click the map or choose “Create here”.",
                "Вейпоинтов пока нет. ПКМ на карте или «Создать здесь».",
                "Aucun repère. Faites un clic droit sur la carte ou choisissez « Créer ici ».",
                "Aún no hay marcadores. Haz clic derecho en el mapa o elige «Crear aquí».",
                "Noch keine Wegpunkte. Rechtsklick auf die Karte oder „Hier erstellen“ wählen.",
            ),
            [16.0, 50.0],
            0xaaaaaa,
            true,
        );
    }
    let mut biome_tooltip = None;
    for (index, waypoint) in state.waypoints.iter().enumerate() {
        let Some(row) = waypoint_row_at(state, batch.width, batch.height, index) else {
            continue;
        };
        let selected = state.waypoint_selected == Some(index);
        if selected {
            game.native_fill(batch, row, 0xff2e4a3a);
        } else if inside(mouse, row) {
            game.native_fill(batch, row, 0xff222b35);
        }
        dimension_icon(
            game,
            batch,
            waypoint.dimension,
            [row[0] + 5.0, row[1] + 4.0],
        );
        let spot = Spot {
            dimension: waypoint.dimension,
            x: waypoint.x,
            z: waypoint.z,
        };
        let biome = state.waypoint_biomes.get(&spot).map(String::as_str);
        biome_icon(game, batch, biome, [row[0] + 18.0, row[1] + 4.0]);
        game.native_text(
            batch,
            &waypoint.name,
            [row[0] + 32.0, row[1] + 3.0],
            0xffffff,
            false,
        );
        game.native_text(
            batch,
            &format!(
                "{}  {} {} {}",
                waypoint.dimension.label(),
                waypoint.x,
                waypoint.y,
                waypoint.z
            ),
            [row[0] + 32.0, row[1] + 15.0],
            0xaaaaaa,
            false,
        );
        if inside(mouse, [row[0] + 17.0, row[1] + 3.0, 10.0, 10.0]) {
            let label = biome.map_or_else(
                || tr("Biome: —", "биом: —", "Biome : —", "Bioma: —", "Biom: —").to_owned(),
                localized_biome,
            );
            biome_tooltip = Some(label);
        }
    }
    let bottom = batch.height - 42.0;
    let selected = state
        .waypoint_selected
        .and_then(|index| state.waypoints.get(index));
    let [rename, delete, show] = waypoint_actions(batch.width, batch.height);
    button(
        game,
        batch,
        rename,
        tr(
            "Rename",
            "Переименовать",
            "Renommer",
            "Cambiar nombre",
            "Umbenennen",
        ),
        mouse,
        selected.is_some(),
    );
    button(
        game,
        batch,
        delete,
        tr("Delete", "Удалить", "Supprimer", "Eliminar", "Löschen"),
        mouse,
        selected.is_some(),
    );
    button(
        game,
        batch,
        show,
        tr(
            "Show on map",
            "Показать на карте",
            "Voir sur la carte",
            "Ver en el mapa",
            "Auf Karte zeigen",
        ),
        mouse,
        selected.is_some(),
    );
    let hint = tr(
        "Select a waypoint to see its name and coordinates",
        "Выберите точку, чтобы увидеть её имя и координаты",
        "Sélectionnez un repère pour voir son nom et ses coordonnées",
        "Selecciona un marcador para ver su nombre y coordenadas",
        "Wegpunkt auswählen, um Namen und Koordinaten anzuzeigen",
    );
    game.native_text(batch, hint, [12.0, bottom + 24.0], 0x888f98, true);
    draw_notice(game, batch, state);
    if let Some(label) = biome_tooltip {
        draw_tooltip(game, batch, &label, mouse);
    }
}

fn draw_name_form(game: &Game<'_>, batch: &mut HudBatch, state: &State, form: &NameForm) {
    match form.background {
        Mode::Map => draw_map_screen(game, batch, state),
        Mode::Waypoints => draw_waypoints_screen(game, batch, state),
        Mode::Name => {}
    }
    game.native_fill(batch, [0.0, 0.0, batch.width, batch.height], 0x70000000);
    let width = 340.0_f32.min(batch.width - 24.0);
    let rect = [
        (batch.width - width) / 2.0,
        batch.height / 2.0 - 45.0,
        width,
        90.0,
    ];
    game.native_fill(batch, rect, 0xf01b2028);
    game.native_fill(batch, [rect[0], rect[1], rect[2], 1.0], 0xff3a4553);
    game.native_fill(
        batch,
        [rect[0], rect[1] + rect[3] - 1.0, rect[2], 1.0],
        0xff3a4553,
    );
    game.native_text(
        batch,
        if form.rename.is_some() {
            tr(
                "Rename waypoint",
                "Новое имя вейпоинта",
                "Renommer le repère",
                "Cambiar nombre del marcador",
                "Wegpunkt umbenennen",
            )
        } else {
            tr(
                "Waypoint name",
                "Название вейпоинта",
                "Nom du repère",
                "Nombre del marcador",
                "Wegpunktname",
            )
        },
        [rect[0] + 10.0, rect[1] + 8.0],
        0xffffff,
        true,
    );
    let field = [rect[0] + 10.0, rect[1] + 26.0, rect[2] - 20.0, 18.0];
    game.native_fill(batch, field, 0xff0d1117);
    game.native_text(
        batch,
        &format!("{}_", form.text),
        [field[0] + 4.0, field[1] + 5.0],
        0xffffff,
        false,
    );
    let mouse = game.native_cursor();
    let save = [
        rect[0] + rect[2] - 184.0,
        rect[1] + rect[3] - 24.0,
        80.0,
        18.0,
    ];
    let cancel = [
        rect[0] + rect[2] - 96.0,
        rect[1] + rect[3] - 24.0,
        80.0,
        18.0,
    ];
    button(
        game,
        batch,
        save,
        tr("Save", "Сохранить", "Enregistrer", "Guardar", "Speichern"),
        mouse,
        true,
    );
    button(
        game,
        batch,
        cancel,
        tr("Cancel", "Отмена", "Annuler", "Cancelar", "Abbrechen"),
        mouse,
        true,
    );
    game.native_text(
        batch,
        tr(
            "Enter — save, Esc — cancel",
            "Enter — сохранить, Esc — отмена",
            "Entrée — enregistrer, Échap — annuler",
            "Intro — guardar, Esc — cancelar",
            "Eingabe — speichern, Esc — abbrechen",
        ),
        [rect[0] + 10.0, rect[1] + rect[3] - 19.0],
        0x888f98,
        false,
    );
}

fn draw_tooltip(game: &Game<'_>, batch: &mut HudBatch, text: &str, at: [f32; 2]) {
    let width = game.native_text_width(text);
    let x = (at[0] + 10.0).min(batch.width - width - 10.0).max(4.0);
    let y = (at[1] - 18.0).max(4.0);
    game.native_fill(batch, [x - 4.0, y - 3.0, width + 8.0, 13.0], 0xf01b2028);
    game.native_text(batch, text, [x, y], 0xffffd24a, true);
}

// The first letter and name of the waypoint under the pointer on the large map.
fn waypoint_at_pointer<'a>(
    state: &'a State,
    view: &View,
    rect: [f32; 4],
    mouse: [f32; 2],
) -> Option<&'a Waypoint> {
    state.waypoints.iter().find(|waypoint| {
        if waypoint.dimension != view.dimension {
            return false;
        }
        let pixel = view.screen_at([f64::from(waypoint.x) + 0.5, f64::from(waypoint.z) + 0.5]);
        let x = rect[0] + pixel[0] as f32 * rect[2] / view.width as f32;
        let y = rect[1] + pixel[1] as f32 * rect[3] / view.height as f32;
        (mouse[0] - x).abs() <= 7.0 && (mouse[1] - y).abs() <= 7.0
    })
}

fn portal_near(state: &State, dimension: Dimension, block: [i32; 2]) -> Option<(Portal, [i32; 3])> {
    state
        .portals
        .iter()
        .filter(|(at_dimension, at, _)| {
            *at_dimension == dimension
                && (at[0] - block[0]).abs() <= 2
                && (at[2] - block[1]).abs() <= 2
        })
        .min_by_key(|(_, at, _)| {
            let dx = at[0] - block[0];
            let dz = at[2] - block[1];
            dx * dx + dz * dz
        })
        .map(|(_, at, kind)| (*kind, *at))
}

fn draw_border(game: &Game<'_>, batch: &mut HudBatch, view: &View, rect: [f32; 4]) {
    if !matches!(view.dimension, Dimension::Overworld | Dimension::Nether) {
        return;
    }
    let [mut min_x, mut min_z, mut max_x, mut max_z] = game.border.bounds();
    if view.dimension == Dimension::Nether {
        min_x /= 8.0;
        min_z /= 8.0;
        max_x /= 8.0;
        max_z /= 8.0;
    }
    let corners = [
        [min_x, min_z],
        [max_x, min_z],
        [max_x, max_z],
        [min_x, max_z],
    ];
    let points: Vec<[f32; 2]> = corners
        .iter()
        .map(|corner| {
            let pixel = view.screen_at(*corner);
            [
                rect[0] + pixel[0] as f32 * rect[2] / view.width as f32,
                rect[1] + pixel[1] as f32 * rect[3] / view.height as f32,
            ]
        })
        .collect();
    for index in 0..4 {
        draw_line(
            game,
            batch,
            points[index],
            points[(index + 1) % 4],
            rect,
            0xffe04040,
        );
    }
}

fn draw_line(
    game: &Game<'_>,
    batch: &mut HudBatch,
    mut from: [f32; 2],
    mut to: [f32; 2],
    rect: [f32; 4],
    color: u32,
) {
    let [x0, y0, x1, y1] = [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]];
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let mut t0 = 0.0_f32;
    let mut t1 = 1.0_f32;
    for (p, q) in [
        (-dx, from[0] - x0),
        (dx, x1 - from[0]),
        (-dy, from[1] - y0),
        (dy, y1 - from[1]),
    ] {
        if p.abs() < f32::EPSILON {
            if q < 0.0 {
                return;
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            if r > t1 {
                return;
            }
            if r > t0 {
                t0 = r;
            }
        } else {
            if r < t0 {
                return;
            }
            if r < t1 {
                t1 = r;
            }
        }
    }
    from = [from[0] + t0 * dx, from[1] + t0 * dy];
    to = [from[0] + (t1 - t0) * dx, from[1] + (t1 - t0) * dy];
    let steps = ((to[0] - from[0]).abs().max((to[1] - from[1]).abs()) / 2.0).ceil() as i32;
    for step in 0..=steps {
        let t = step as f32 / steps.max(1) as f32;
        game.native_fill(
            batch,
            [
                from[0] + (to[0] - from[0]) * t,
                from[1] + (to[1] - from[1]) * t,
                2.0,
                2.0,
            ],
            color,
        );
    }
}

fn project(
    camera: &horizon_client::mod_api::NativeCamera,
    world: [f64; 3],
    size: [f32; 2],
) -> Option<[f32; 2]> {
    let relative = [
        (world[0] - camera.position[0]) as f32,
        (world[1] - camera.position[1]) as f32,
        (world[2] - camera.position[2]) as f32,
        1.0,
    ];
    let transform = |matrix: [f32; 16], vector: [f32; 4]| {
        [
            matrix[0] * vector[0]
                + matrix[4] * vector[1]
                + matrix[8] * vector[2]
                + matrix[12] * vector[3],
            matrix[1] * vector[0]
                + matrix[5] * vector[1]
                + matrix[9] * vector[2]
                + matrix[13] * vector[3],
            matrix[2] * vector[0]
                + matrix[6] * vector[1]
                + matrix[10] * vector[2]
                + matrix[14] * vector[3],
            matrix[3] * vector[0]
                + matrix[7] * vector[1]
                + matrix[11] * vector[2]
                + matrix[15] * vector[3],
        ]
    };
    let view = transform(camera.view, relative);
    let clip = transform(camera.projection, view);
    if clip[3] <= camera.near {
        return None;
    }
    let x = clip[0] / clip[3];
    let y = clip[1] / clip[3];
    Some([(x * 0.5 + 0.5) * size[0], (1.0 - (y * 0.5 + 0.5)) * size[1]])
}

fn draw_world_waypoints(game: &Game<'_>, batch: &mut HudBatch, state: &State) {
    let Some(player) = game.native_player() else {
        return;
    };
    let Some(camera) = game.native_camera() else {
        return;
    };
    let dimension = Dimension::from_id(player.dimension);
    for waypoint in &state.waypoints {
        if waypoint.dimension != dimension {
            continue;
        }
        let world = [
            f64::from(waypoint.x) + 0.5,
            f64::from(waypoint.y) + 1.5,
            f64::from(waypoint.z) + 0.5,
        ];
        let Some([x, y]) = project(&camera, world, [batch.width, batch.height]) else {
            continue;
        };
        if x < -8.0 || y < -8.0 || x > batch.width + 8.0 || y > batch.height + 8.0 {
            continue;
        }
        game.native_fill(batch, [x - 6.0, y - 6.0, 12.0, 12.0], 0xff101010);
        game.native_fill(batch, [x - 5.0, y - 5.0, 10.0, 10.0], 0xffffd24a);
        let letter = first_letter(&waypoint.name).to_string();
        game.native_text(
            batch,
            &letter,
            [x - game.native_text_width(&letter) / 2.0, y - 4.0],
            0xff201800,
            false,
        );
    }
}

// A frame may advance only within the atlas that is actually uploaded. Keep
// the last displayed camera while a wider atlas is being prepared, but always
// pair the chosen camera with the *new* texture's metadata after an upload.
fn complete_map_view(
    atlas: &maps::TerrainAtlas,
    desired: View,
    previous: Option<&View>,
    prepared: &View,
) -> View {
    if desired.session != prepared.session
        || desired.dimension != prepared.dimension
        || desired.layer != prepared.layer
        || desired.large != prepared.large
    {
        return prepared.clone();
    }
    if atlas.covers(&desired) {
        return desired;
    }
    if let Some(previous) = previous
        && previous.session == desired.session
        && previous.dimension == desired.dimension
        && previous.layer == desired.layer
        && atlas.covers(previous)
    {
        return previous.clone();
    }
    prepared.clone()
}

pub(super) fn frame(game: &mut Game<'_>, elapsed: Duration) {
    if !game.native_window() {
        return;
    }
    let controller = runtime::initialize(game.native_data_dir(runtime::MOD_ID));
    if !game.native_on(runtime::MOD_ID) {
        if game.native_mod_screen() == Some(runtime::MOD_ID) {
            game.native_mod_screen_close(runtime::MOD_ID);
        }
        return;
    }
    let Some(player) = game.native_player() else {
        return;
    };
    let Some(identity) = game.native_client_state() else {
        return;
    };
    let (copy, command) = {
        let mut state = state();
        (state.pending_copy.take(), state.pending_command.take())
    };
    if let Some(text) = copy {
        use horizon_client::clipboard::Clipboard;
        game.clipboard.set(&text);
    }
    if let Some(command) = command {
        game.click_command(&command);
    }
    let mut state = state();
    if state.close {
        state.close = false;
        runtime::set_text_input(false);
        game.native_mod_screen_close(runtime::MOD_ID);
    }
    if game.native_mod_screen() != Some(runtime::MOD_ID) {
        runtime::set_text_input(false);
        if state.mode == Mode::Name {
            state.mode = Mode::Map;
            state.name_form = None;
        }
    }
    if let Some(request) = runtime::take_screen_request() {
        let desired = match request {
            runtime::ScreenRequest::Map => Mode::Map,
            runtime::ScreenRequest::Waypoints => Mode::Waypoints,
        };
        // The key helper opens the screen; this side only chooses its mode.
        if game.native_mod_screen() == Some(runtime::MOD_ID) {
            state.mode = desired;
            state.info = None;
            state.name_form = None;
            state.menu = None;
            state.waypoint_selected = None;
            runtime::set_text_input(false);
        }
    }
    state.portals = controller.maps.portals();
    use super::native_client_services::NativeWorldId;
    let world = match identity.world.as_ref() {
        Some(NativeWorldId::Singleplayer(folder)) => runtime::waypoint_world(true, folder),
        Some(NativeWorldId::Server(address)) => runtime::waypoint_world(false, address),
        _ => None,
    }
    .unwrap_or_default();
    if state.world != world {
        let next = state.session + 1;
        *state = State {
            world,
            session: next,
            ..State::default()
        };
        controller.maps.reset(next);
    }
    state.waypoints = controller.waypoints(&state.world);
    let dimension = Dimension::from_id(player.dimension);
    if state.dimension != dimension {
        state.seen.clear();
        state.dimension = dimension;
    }
    let head = (player.y + 1.62).floor() as i32;
    let covered = game
        .native_light([player.x.floor() as i32, head, player.z.floor() as i32])
        .1
        == 0;
    let layer = if dimension == Dimension::Nether || covered {
        Layer::Below(head)
    } else {
        Layer::Surface
    };
    state.layers.insert(dimension, layer);
    let settings = controller.settings();
    let screen_open = game.native_mod_screen() == Some(runtime::MOD_ID);
    let large = screen_open && state.mode == Mode::Map;
    let selected = if large {
        settings.world_dimension
    } else {
        settings.mini_dimension
    }
    .resolve(dimension);
    let position = dimension.convert(selected, [player.x, player.z]);
    if selected != state.selected || large != state.large {
        state.camera.snap(position);
        state.marker = position;
        state.selected = selected;
        state.large = large;
        state.dragging = false;
        state.menu = None;
    }
    if !large {
        state.camera.follow(position);
    }
    apply_map_focus(&mut state, large, selected);
    let dt = elapsed.as_secs_f64();
    state.camera.advance(dt);
    state.marker = maps::follow_point(state.marker, position, dt);
    state.lod_elapsed += dt;
    if state.lod_elapsed >= 0.25 {
        state.lod_elapsed = 0.0;
        if let Some(lod) = game.lod.as_ref() {
            let coverage = controller.maps.coverage();
            let known = coverage
                .lod
                .iter()
                .filter(|(d, _, _)| *d == dimension)
                .filter_map(|(_, key, _)| {
                    state
                        .lod_sources
                        .get(&(dimension, *key))
                        .map(|(source, _)| ((key.x, key.z, key.level), source.as_ptr() as usize))
                })
                .collect();
            for (key, columns) in horizon_client::lod::minimap_lod_adapter::snapshots(
                lod,
                &known,
                state.camera.center,
                4,
            ) {
                let source = Arc::downgrade(&columns);
                let biome_palette = game
                    .world_biomes
                    .as_ref()
                    .unwrap_or_else(|| game.res.biomes())
                    .clone();
                let resources = Arc::clone(&game.res);
                let key = maps::LodKey {
                    x: key.x,
                    z: key.z,
                    level: key.level,
                };
                let accepted =
                    controller
                        .maps
                        .submit_lod(state.session, dimension, key, move || {
                            let mut biome_names = BTreeMap::<u16, Arc<str>>::new();
                            let book = resources.facts();
                            let color = |id: horizon_world::registry::BlockStateId,
                                         biome: u16,
                                         at: [i64; 2]| {
                                let state = book.states.get(id.0 as usize)?;
                                if book.is_air(id) {
                                    return None;
                                }
                                let base = plant_color(state.name.as_ref()).unwrap_or_else(|| {
                                    horizon_client::blocks::MAP_COLORS
                                        .get(state.map_color as usize)
                                        .copied()
                                        .unwrap_or(0)
                                });
                                let grass = biome_palette
                                    .list
                                    .get(usize::from(biome))
                                    .map(|_| {
                                        biome_palette.grass(biome, at[0] as f64, at[1] as f64)
                                    });
                                Some(maps::block_map_color(state.name.as_ref(), base, grass))
                            };
                            let mut tile = maps::Tile {
                                pixels: vec![0; 1024],
                                biomes: vec![None; 256],
                                ..maps::Tile::default()
                            };
                            let [origin_x, origin_z, size, _] = key.bounds();
                            let stride = size / 16;
                            for (i, column) in columns.iter().enumerate() {
                                let Some(surface) = column.surface else {
                                    continue;
                                };
                                let at = [
                                    origin_x + (i as i64 % 16) * stride + stride / 2,
                                    origin_z + (i as i64 / 16) * stride + stride / 2,
                                ];
                                let Some(mut rgb) = color(surface.state, surface.biome, at) else {
                                    continue;
                                };
                                if let Some((height, water)) = surface.water
                                    && let Some(water) = color(water, surface.biome, at)
                                {
                                    let depth = (height - surface.height).max(0);
                                    let alpha = (90 + depth * 8).clamp(90, 210) as u32;
                                    let channel = |shift: u32| -> u32 {
                                        (((rgb >> shift) & 255_u32) * (255 - alpha)
                                            + ((water >> shift) & 255_u32) * alpha)
                                            / 255
                                    };
                                    rgb = (channel(16) << 16) | (channel(8) << 8) | channel(0);
                                }
                                if let Some(info) = biome_palette.list.get(surface.biome as usize) {
                                    let name = biome_names
                                        .entry(surface.biome)
                                        .or_insert_with(|| Arc::from(info.name.as_str()));
                                    tile.biomes[i] = Some(name.clone());
                                }
                                tile.heights[i] = surface
                                    .water
                                    .map(|(height, _)| height)
                                    .unwrap_or(surface.height)
                                    .saturating_sub(1);
                                tile.pixels[i * 4..i * 4 + 4].copy_from_slice(&[
                                    (rgb >> 16) as u8,
                                    (rgb >> 8) as u8,
                                    rgb as u8,
                                    255,
                                ]);
                            }
                            tile
                        });
                if accepted {
                    state
                        .lod_sources
                        .insert((dimension, key), (source, Instant::now()));
                }
            }
        }
    }
    state.elapsed += dt;
    if state.elapsed >= 0.05 {
        state.elapsed = 0.0;
        let known: Vec<_> = state
            .waypoints
            .iter()
            .filter_map(|waypoint| {
                known_biome(
                    game,
                    waypoint.dimension,
                    [waypoint.x, waypoint.y, waypoint.z],
                )
                .map(|biome| {
                    (
                        Spot {
                            dimension: waypoint.dimension,
                            x: waypoint.x,
                            z: waypoint.z,
                        },
                        biome,
                    )
                })
            })
            .collect();
        let spots: Vec<_> = state
            .waypoints
            .iter()
            .map(|w| Spot {
                dimension: w.dimension,
                x: w.x,
                z: w.z,
            })
            .collect();
        state.waypoint_biomes.retain(|spot, _| spots.contains(spot));
        state.waypoint_biomes.extend(known);
        let cx = (player.x.floor() as i32).div_euclid(16);
        let cz = (player.z.floor() as i32).div_euclid(16);
        let mut chunks: Vec<_> = game
            .world
            .chunks
            .iter()
            .map(|(&pos, chunk)| (pos, Arc::downgrade(chunk)))
            .collect();
        let requested = controller.maps.requested();
        chunks.sort_by_key(|(pos, _)| {
            (
                !requested
                    .as_ref()
                    .is_some_and(|v| v.demands(dimension, layer, [pos.0, pos.1])),
                i64::from(pos.0 - cx).pow(2) + i64::from(pos.1 - cz).pow(2),
            )
        });
        state
            .seen
            .retain(|pos, _| game.world.chunks.contains_key(pos));
        let mut sent = 0;
        for (pos, weak) in chunks {
            let visible = requested
                .as_ref()
                .is_some_and(|v| v.demands(dimension, layer, [pos.0, pos.1]));
            if state.seen.get(&pos).is_some_and(|(old, old_layer, sent)| {
                Weak::ptr_eq(old, &weak)
                    && *old_layer == layer
                    && (!visible
                        || sent.elapsed() < Duration::from_millis(500)
                        || controller.maps.contains(dimension, layer, [pos.0, pos.1]))
            }) {
                continue;
            }
            let Ok(snapshot) = game.native_client_chunks([pos.0, pos.1], [pos.0, pos.1]) else {
                continue;
            };
            let resources = Arc::clone(&game.res);
            let biome_palette = game
                .world_biomes
                .as_ref()
                .unwrap_or_else(|| game.res.biomes())
                .clone();
            let accepted = controller.maps.submit(
                state.session,
                dimension,
                layer,
                [pos.0, pos.1],
                move || {
                    let book = resources.facts();
                    let minimum = snapshot.height().start;
                    maps::sample(
                        |x, y, z| {
                            let id = snapshot.block([x, y, z]);
                            let air = book.is_air(id);
                            let Some(state) = book.states.get(id.0 as usize) else {
                                return maps::Block {
                                    rgb: 0,
                                    air,
                                    alpha: 0,
                                    hidden: false,
                                    water: false,
                                    portal: None,
                                };
                            };
                            let hidden = hidden_cover(state.classes.as_ref(), state.name.as_ref());
                            let portal = match state.name.as_ref() {
                                "minecraft:nether_portal" => Some(Portal::Nether),
                                "minecraft:end_portal" => Some(Portal::End),
                                "minecraft:end_gateway" => Some(Portal::Gateway),
                                _ => None,
                            };
                            if air || hidden {
                                return maps::Block {
                                    rgb: 0,
                                    air,
                                    alpha: 0,
                                    hidden,
                                    water: false,
                                    portal: None,
                                };
                            }
                            if let Some(kind) = portal {
                                let (rgb, alpha) = portal_tint(kind);
                                return maps::Block {
                                    rgb,
                                    air,
                                    alpha,
                                    hidden: false,
                                    water: false,
                                    portal: Some(kind),
                                };
                            }
                            let water = matches!(state.fluid, horizon_client::blocks::Fluid::Water);
                            let fluid = !matches!(state.fluid, horizon_client::blocks::Fluid::None);
                            let invisible = matches!(
                                state.render_shape,
                                horizon_client::blocks::RenderShape::Invisible
                            );
                            if invisible && !fluid {
                                // Barriers, light blocks, moving pistons and
                                // other render-invisible blocks leave no pixel.
                                return maps::Block {
                                    rgb: 0,
                                    air,
                                    alpha: 0,
                                    hidden: true,
                                    water: false,
                                    portal: None,
                                };
                            }
                            let base = horizon_client::blocks::MAP_COLORS
                                .get(state.map_color as usize)
                                .copied()
                                .unwrap_or(0);
                            let grass = matches!(
                                state.name.as_ref(),
                                "minecraft:grass_block" | "minecraft:grass"
                            )
                            .then(|| {
                                snapshot.biome([x, y, z]).map(|biome| {
                                    biome_palette.grass(
                                        biome.0,
                                        f64::from(x),
                                        f64::from(z),
                                    )
                                })
                            })
                            .flatten();
                            let rgb = plant_color(state.name.as_ref()).unwrap_or_else(|| {
                                maps::block_map_color(state.name.as_ref(), base, grass)
                            });
                            let glass = state.classes.iter().any(|class| {
                                class.as_ref() == "TransparentBlock"
                                    || class.as_ref() == "HalfTransparentBlock"
                            }) || state.name.contains("glass");
                            let soft = matches!(
                                state.name.as_ref(),
                                "minecraft:scaffolding" | "minecraft:cobweb"
                            );
                            let (rgb, alpha) = if water {
                                (if rgb == 0 { 0x3f76e4 } else { rgb }, 120)
                            } else if glass {
                                let tint = if rgb == 0 {
                                    dye_color(state.name.as_ref()).unwrap_or(0xbfd8e6)
                                } else {
                                    rgb
                                };
                                let alpha = match state.name.as_ref() {
                                    "minecraft:tinted_glass" => 190,
                                    "minecraft:ice"
                                    | "minecraft:blue_ice"
                                    | "minecraft:frosted_ice" => 130,
                                    _ if state.name.contains("glass_pane") => 80,
                                    _ => 96,
                                };
                                (tint, alpha)
                            } else if soft {
                                (if rgb == 0 { 0xbfb8a8 } else { rgb }, 90)
                            } else {
                                (rgb, 0)
                            };
                            maps::Block {
                                rgb,
                                air,
                                alpha,
                                hidden: false,
                                water,
                                portal: None,
                            }
                        },
                        |x, z| snapshot.section_top([x, z]),
                        [pos.0, pos.1],
                        minimum,
                        layer,
                    )
                },
            );
            if !accepted {
                break;
            }
            state.seen.insert(pos, (weak, layer, Instant::now()));
            sent += 1;
            if sent == 4 {
                break;
            }
        }
    }
    let mut current_view = None;
    if let Some(renderer) = game.renderer.as_ref() {
        let viewport = HudBatch::new([renderer.config.width, renderer.config.height]);
        let rect = rectangle(viewport.width, viewport.height);
        let (width, height, span) = if large {
            // The HUD draws in GUI pixels, but the texture is sampled at the
            // window's physical resolution. Keep this coordinate space for
            // overlays; terrain tiles are now rasterized directly by the GPU.
            let scale = viewport.scale.max(1.0);
            (
                ((rect[2] * scale).ceil() as usize).max(1),
                ((rect[3] * scale).ceil() as usize).max(1),
                [
                    f64::from(rect[2]) * state.camera.zoom,
                    f64::from(rect[3]) * state.camera.zoom,
                ],
            )
        } else {
            let size = settings
                .fitted_size(viewport.width, viewport.height)
                .unwrap_or(128.0);
            let texture = (size * 4.0).round().clamp(64.0, 1024.0) as usize;
            (
                texture,
                texture,
                [f64::from(settings.blocks_per_pixel) * 128.0; 2],
            )
        };
        let selected_layer = state
            .layers
            .get(&selected)
            .copied()
            .unwrap_or(Layer::Surface);
        let view = View {
            session: state.session,
            dimension: selected,
            layer: selected_layer,
            center: state.camera.center,
            span,
            width,
            height,
            angle: if !large && settings.rotate_with_player && selected == dimension {
                (f64::from(player.yaw) + 180.0).to_radians()
            } else {
                0.0
            },
            round: !large && settings.round,
            large,
            smooth: large && settings.smooth_worldmap,
        };
        controller.maps.request(view.clone());
        current_view = Some(view);
    }
    if let Some(image) = controller.maps.frame()
        && image.view.session == state.session
        && image.view.dimension == selected
        && image.view.large == large
    {
        let (serial, width, height, pixels) = match &image.atlas {
            Some(atlas) => (
                (atlas.serial, atlas.width),
                atlas.width,
                atlas.height,
                &atlas.pixels,
            ),
            None => (
                (image.serial, 0),
                image.view.width,
                image.view.height,
                &image.pixels,
            ),
        };
        if serial != state.uploaded && !game.native_mod_image(width as u32, height as u32, pixels) {
            return;
        }
        state.uploaded = serial;
        // Camera movement is independent of worker sampling/upload latency.
        // Pointer queries and overlays use this exact same displayed view.
        if let (Some(atlas), Some(view)) = (&image.atlas, current_view) {
            if view.layer == image.view.layer {
                let view = complete_map_view(
                    atlas,
                    view,
                    state.frame.as_ref().map(|frame| &frame.view),
                    &image.view,
                );
                state.frame = Some(Arc::new(maps::Frame {
                    serial: image.serial,
                    view,
                    pixels: Vec::new(),
                    known: image.known,
                    atlas: Some(atlas.clone()),
                }));
            }
        } else {
            state.frame = Some(image);
        }
    }
}

pub(super) fn hud(game: &mut Game<'_>, batch: &mut HudBatch, corners: &mut Corners) {
    if !game.native_window()
        || !game.native_on(runtime::MOD_ID)
        || game.native_player().is_none()
        || game.native_mod_screen().is_some()
    {
        return;
    }
    let settings = runtime::initialize(game.native_data_dir(runtime::MOD_ID)).settings();
    if game.native_minimap_now().is_none() && settings.enabled {
        if let Some(size) = settings.fitted_size(batch.width, batch.height) {
            let corner = match settings.corner {
                runtime::minimap::Corner::TopLeft => Corner::TopLeft,
                runtime::minimap::Corner::TopRight => Corner::TopRight,
                runtime::minimap::Corner::BottomLeft => Corner::BottomLeft,
                runtime::minimap::Corner::BottomRight => Corner::BottomRight,
            };
            if game.native_corner_shown(corner) {
                let state = state();
                let lines = game.native_player().map_or_else(Vec::new, |player| {
                    let dimension = match player.dimension {
                        "minecraft:overworld" => Dimension::Overworld,
                        "minecraft:the_nether" => Dimension::Nether,
                        _ => return Vec::new(),
                    };
                    runtime::coordinate_lines(settings, dimension, [player.x, player.y, player.z])
                });
                let footer = if lines.is_empty() {
                    0.0
                } else {
                    lines.len() as f32 * 12.0 + 2.0
                };
                let width = lines
                    .iter()
                    .map(|line| game.native_text_width(line))
                    .fold(size, f32::max);
                let (x, y) = corners.place(corner, width, size + footer);
                let map_x = if matches!(corner, Corner::TopRight | Corner::BottomRight) {
                    x + width - size
                } else {
                    x
                };
                draw(game, batch, &state, [map_x, y, size, size], false, None);
                for (index, line) in lines.iter().enumerate() {
                    game.native_text(
                        batch,
                        line,
                        [x, y + size + 2.0 + index as f32 * 12.0],
                        0xdddddd,
                        true,
                    );
                }
            }
        }
    }
    let state = state();
    draw_world_waypoints(game, batch, &state);
}

fn draw(
    game: &Game<'_>,
    batch: &mut HudBatch,
    state: &State,
    rect: [f32; 4],
    large: bool,
    mouse: Option<[f32; 2]>,
) {
    let round = !large
        && runtime::initialize(game.native_data_dir(runtime::MOD_ID))
            .settings()
            .round;
    if !round {
        game.native_fill(batch, rect, 0xef181b20);
    }
    let valid = state.frame.as_ref().filter(|f| {
        f.view.dimension == state.selected
            && f.view.large == large
            && f.view.session == state.session
    });
    if let Some(frame) = valid {
        let mut terrain_visible = frame.known > 0;
        if let Some(atlas) = &frame.atlas {
            terrain_visible = false;
            for coarse in &atlas.coarse {
                for part in &coarse.fragments {
                    if let Some((src, dst)) = atlas.coarse_quad(&frame.view, rect, coarse, *part) {
                        game.native_sprite(batch, horizon_client::hud::MOD_IMAGE_SPRITE, src, dst);
                        terrain_visible = true;
                    }
                }
            }
            for (slot, chunk) in atlas.chunks.iter().enumerate() {
                if let Some((src, dst)) = atlas.quad(&frame.view, rect, *chunk, slot) {
                    game.native_sprite(batch, horizon_client::hud::MOD_IMAGE_SPRITE, src, dst);
                    terrain_visible = true;
                }
            }
        } else {
            game.native_sprite(
                batch,
                horizon_client::hud::MOD_IMAGE_SPRITE,
                [0.0, 0.0, frame.view.width as f32, frame.view.height as f32],
                rect,
            );
        }
        draw_border(game, batch, &frame.view, rect);
        let mut drawn: Vec<[i32; 2]> = Vec::new();
        for (dimension, at, kind) in &state.portals {
            if *dimension != frame.view.dimension {
                continue;
            }
            if drawn
                .iter()
                .any(|point| (point[0] - at[0]).abs() <= 2 && (point[1] - at[2]).abs() <= 2)
            {
                continue;
            }
            drawn.push([at[0], at[2]]);
            draw_portal_marker(game, batch, &frame.view, rect, *at, *kind);
        }
        if large {
            let hover = state
                .menu
                .map(|menu| menu.block)
                .or_else(|| mouse.and_then(|mouse| hovered_block(&frame.view, rect, mouse)));
            if let Some(block) = hover {
                highlight(game, batch, &frame.view, rect, block);
            }
        }
        for waypoint in state
            .waypoints
            .iter()
            .filter(|waypoint| waypoint.dimension == frame.view.dimension)
        {
            draw_waypoint(
                game,
                batch,
                &frame.view,
                rect,
                waypoint,
                if large { 11.0 } else { 8.0 },
            );
        }
        if let Some(player) = game.native_player() {
            draw_marker(game, batch, &frame.view, rect, state.marker, &player);
        }
        if terrain_visible {
            return;
        }
    }
    let text = if valid.is_some() {
        tr(
            "No data",
            "Нет данных",
            "Aucune donnée",
            "Sin datos",
            "Keine Daten",
        )
    } else {
        tr(
            "Preparing map…",
            "Подготовка карты…",
            "Préparation de la carte…",
            "Preparando mapa…",
            "Karte wird vorbereitet…",
        )
    };
    game.native_text(
        batch,
        text,
        [
            rect[0] + (rect[2] - game.native_text_width(text)) / 2.0,
            rect[1] + rect[3] / 2.0 - 4.0,
        ],
        0xcccccc,
        true,
    );
}

fn tab(width: f32, index: usize) -> [f32; 4] {
    let w = ((width - 16.0) / 4.0).max(1.0);
    [8.0 + w * index as f32, 8.0, (w - 2.0).max(1.0), 18.0]
}

fn information_rect(width: f32, height: f32) -> [f32; 4] {
    let w = (width - 24.0).clamp(1.0, 560.0);
    let h = (height - 24.0).clamp(1.0, 260.0);
    [(width - w) / 2.0, (height - h) / 2.0, w, h]
}
fn information_lines(game: &Game<'_>, state: &State, menu: Menu, width: f32) -> Vec<(String, u32)> {
    let layer = state
        .frame
        .as_ref()
        .filter(|frame| frame.view.dimension == menu.dimension)
        .map(|f| f.view.layer)
        .or_else(|| state.layers.get(&menu.dimension).copied())
        .unwrap_or(Layer::Surface);
    let coverage = runtime::initialize(game.native_data_dir(runtime::MOD_ID))
        .maps
        .coverage();
    let (chunk, region) = maps::chunk_region(menu.block);
    let chunk_rect = [i64::from(chunk[0]) * 16, i64::from(chunk[1]) * 16, 16, 16];
    let region_rect = [
        i64::from(region[0]) * 512,
        i64::from(region[1]) * 512,
        512,
        512,
    ];
    let detail = coverage.detailed_at(menu.dimension, layer, menu.block);
    let lod_sample = coverage.lod_sample(menu.dimension, layer, menu.block);
    let lod = lod_sample.as_ref().map(|sample| sample.key);
    let source = if detail {
        tr(
            "Detailed client chunk (cached)",
            "Подробный клиентский чанк (кэш)",
            "Chunk client détaillé (en cache)",
            "Chunk detallado del cliente (caché)",
            "Detaillierter Client-Chunk (Cache)",
        )
    } else if lod.is_some() {
        tr(
            "LOD — approximate data",
            "LOD — приблизительные данные",
            "LOD — données approximatives",
            "LOD — datos aproximados",
            "LOD — ungefähre Daten",
        )
    } else {
        tr(
            "No data for this layer",
            "Нет данных для этого слоя",
            "Aucune donnée pour cette couche",
            "Sin datos para esta capa",
            "Keine Daten für diese Ebene",
        )
    };
    let layer_text = match layer {
        Layer::Surface => tr(
            "Surface",
            "Поверхность",
            "Surface",
            "Superficie",
            "Oberfläche",
        )
        .to_owned(),
        Layer::Below(y) => localized_format!(
            "Below Y {y}",
            "Ниже Y {y}",
            "Sous Y {y}",
            "Por debajo de Y {y}",
            "Unter Y {y}"
        ),
    };
    let normal = 0xffe1e7ed;
    let yellow = 0xffffd45c;
    let mut lines = vec![
        (
            format!("{} — {}", menu.dimension.label(), layer_text),
            normal,
        ),
        (
            format!("X: {}  Z: {}", menu.block[0], menu.block[1]),
            normal,
        ),
        (
            localized_format!(
                "Chunk: {} / {}",
                "Чанк: {} / {}",
                "Chunk : {} / {}",
                "Chunk: {} / {}",
                "Chunk: {} / {}",
                chunk[0],
                chunk[1]
            ),
            normal,
        ),
        (
            format!(
                "X {}..{}   Z {}..{}",
                chunk_rect[0],
                chunk_rect[0] + 15,
                chunk_rect[1],
                chunk_rect[1] + 15
            ),
            normal,
        ),
        (
            source.to_owned(),
            if !detail && lod.is_some() {
                yellow
            } else {
                normal
            },
        ),
    ];
    if let Some(key) = lod {
        let stride = 1_i64 << key.level;
        lines.push((
            localized_format!(
                "LOD: 1 sample per {stride}×{stride} blocks",
                "LOD: 1 отсчёт на {stride}×{stride} блоков",
                "LOD : 1 échantillon pour {stride}×{stride} blocs",
                "LOD: 1 muestra por {stride}×{stride} bloques",
                "LOD: 1 Abtastwert je {stride}×{stride} Blöcke"
            ),
            yellow,
        ));
        let b = key.bounds();
        lines.push((
            format!(
                "LOD: X {}..{}  Z {}..{}",
                b[0],
                b[0] + b[2] - 1,
                b[1],
                b[1] + b[3] - 1
            ),
            normal,
        ));
    }
    if let Some(sample) = lod_sample {
        let biome = sample
            .biome
            .as_ref()
            .map(|id| localized_biome(id))
            .unwrap_or_else(|| {
                tr("Biome: —", "биом: —", "Biome : —", "Bioma: —", "Biom: —").into()
            });
        lines.push((format!("LOD: Y ≈{} | {biome}", sample.height), yellow));
    }
    for (is_region, bounds) in [(false, chunk_rect), (true, region_rect)] {
        let (d, _) = coverage.counts(menu.dimension, layer, bounds);
        let (_, l) = coverage.counts(menu.dimension, Layer::Surface, bounds);
        if is_region {
            lines.push((
                localized_format!(
                    "Region: {} / {} (32×32 chunks)",
                    "Регион: {} / {} (32×32 чанка)",
                    "Région : {} / {} (32×32 chunks)",
                    "Región: {} / {} (32×32 chunks)",
                    "Region: {} / {} (32×32 Chunks)",
                    region[0],
                    region[1]
                ),
                normal,
            ));
            lines.push((
                format!(
                    "X {}..{}   Z {}..{}",
                    bounds[0],
                    bounds[0] + 511,
                    bounds[1],
                    bounds[1] + 511
                ),
                normal,
            ));
        }
        if l > 0 {
            let area = if is_region {
                tr("Region", "Регион", "Région", "Región", "Region")
            } else {
                tr("Chunk", "Чанк", "Chunk", "Chunk", "Chunk")
            };
            let status = if d > 0 {
                tr(
                    "mixed coverage: detailed + LOD",
                    "смешанное покрытие: подробное + LOD",
                    "couverture mixte : détaillée + LOD",
                    "cobertura mixta: detallada + LOD",
                    "gemischte Abdeckung: detailliert + LOD",
                )
            } else {
                tr(
                    "LOD coverage — approximate",
                    "LOD-покрытие — приблизительно",
                    "couverture LOD — approximative",
                    "cobertura LOD — aproximada",
                    "LOD-Abdeckung — ungefähr",
                )
            };
            lines.push((format!("{area}: {status}"), yellow));
            if layer != Layer::Surface {
                lines.push((
                    tr(
                        "Surface LOD cannot describe this underground layer",
                        "Наружный LOD не описывает этот подземный слой",
                        "Le LOD de surface ne décrit pas cette couche souterraine",
                        "El LOD de superficie no describe esta capa subterránea",
                        "Oberflächen-LOD beschreibt diese unterirdische Ebene nicht",
                    )
                    .to_owned(),
                    yellow,
                ));
            }
        }
        if is_region {
            lines.push((
                localized_format!(
                    "Cached detailed chunks: {d}; LOD footprints: {l}",
                    "Подробных чанков в кэше: {d}; LOD-тайлов: {l}",
                    "Chunks détaillés en cache : {d} ; tuiles LOD : {l}",
                    "Chunks detallados en caché: {d}; teselas LOD: {l}",
                    "Detaillierte Chunks im Cache: {d}; LOD-Kacheln: {l}"
                ),
                normal,
            ));
        }
    }
    lines.push((
        tr(
            "Visit history is not recorded yet",
            "История посещений пока не записывается",
            "L’historique des visites n’est pas encore enregistré",
            "El historial de visitas aún no se registra",
            "Der Besuchsverlauf wird noch nicht aufgezeichnet",
        )
        .to_owned(),
        0xff9faebd,
    ));
    let mut wrapped = Vec::new();
    for (text, color) in lines {
        let mut line = String::new();
        for word in text.split_whitespace() {
            let next = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && game.native_text_width(&next) > width {
                wrapped.push((std::mem::take(&mut line), color));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        if !line.is_empty() {
            wrapped.push((line, color));
        }
    }
    wrapped
}
fn draw_information(game: &Game<'_>, batch: &mut HudBatch, state: &State, menu: Menu) {
    game.native_fill(batch, [0.0, 0.0, batch.width, batch.height], 0x98000000);
    let rect = information_rect(batch.width, batch.height);
    game.native_fill(batch, rect, 0xff151d27);
    game.native_text(
        batch,
        tr(
            "Chunk / region",
            "Чанк / регион",
            "Chunk / région",
            "Chunk / región",
            "Chunk / Region",
        ),
        [rect[0] + 10.0, rect[1] + 8.0],
        0xffffff,
        true,
    );
    let lines = information_lines(game, state, menu, rect[2] - 24.0);
    let visible = ((rect[3] - 56.0) / 12.0).max(1.0) as usize;
    let start = state.info_scroll.min(lines.len().saturating_sub(visible));
    for (i, (text, color)) in lines.iter().skip(start).take(visible).enumerate() {
        game.native_text(
            batch,
            text,
            [rect[0] + 10.0, rect[1] + 25.0 + i as f32 * 12.0],
            *color,
            true,
        );
    }
    if lines.len() > visible {
        let area = rect[3] - 56.0;
        let thumb = area * visible as f32 / lines.len() as f32;
        let y = rect[1]
            + 25.0
            + (area - thumb) * start as f32 / lines.len().saturating_sub(visible).max(1) as f32;
        game.native_fill(batch, [rect[0] + rect[2] - 7.0, y, 3.0, thumb], 0xffa2b2bf);
    }
    button(
        game,
        batch,
        [
            rect[0] + rect[2] - 82.0,
            rect[1] + rect[3] - 24.0,
            72.0,
            18.0,
        ],
        tr("Close", "Закрыть", "Fermer", "Cerrar", "Schließen"),
        game.native_cursor(),
        true,
    );
}

fn map_footer(width: f32, height: f32) -> [f32; 4] {
    [
        8.0,
        (height - 54.0).max(0.0),
        (width - 16.0).max(1.0),
        46.0_f32.min(height),
    ]
}
fn over_map_controls(mouse: [f32; 2], width: f32, height: f32) -> bool {
    inside(mouse, map_footer(width, height)) || (0..4).any(|i| inside(mouse, tab(width, i)))
}
fn draw_map_screen(game: &Game<'_>, batch: &mut HudBatch, state: &State) {
    let mouse = game.native_cursor();
    let rect = rectangle(batch.width, batch.height);
    draw(game, batch, state, rect, true, Some(mouse));
    let mut coordinates = tr(
        "Point under cursor: —",
        "Точка под мышью: —",
        "Point sous le curseur : —",
        "Punto bajo el cursor: —",
        "Punkt unter dem Mauszeiger: —",
    )
    .to_owned();
    let mut nether = tr(
        "Nether (cursor): —",
        "Незер (точка под мышью): —",
        "Nether (curseur) : —",
        "Nether (cursor): —",
        "Nether (Mauszeiger): —",
    )
    .to_owned();
    let mut biome = None;
    let mut lod_warning = false;
    let mut tooltip = None;
    if !over_map_controls(mouse, batch.width, batch.height)
        && let Some(frame) = state.frame.as_ref().filter(|f| {
            f.view.dimension == state.selected && f.view.large && f.view.session == state.session
        })
        && let Some(block) = hovered_block(&frame.view, rect, mouse)
    {
        let height = if state.dimension == frame.view.dimension {
            hovered_height(game, frame.view.layer, block)
        } else {
            match frame.view.layer {
                Layer::Below(y) => Some(y),
                Layer::Surface => None,
            }
        };
        let coverage = runtime::initialize(game.native_data_dir(runtime::MOD_ID))
            .maps
            .coverage();
        let rough = if coverage.detailed_at(frame.view.dimension, frame.view.layer, block) {
            None
        } else {
            coverage.lod_sample(frame.view.dimension, frame.view.layer, block)
        };
        let height_approximate = height.is_none() && rough.is_some();
        let height = height.or_else(|| rough.as_ref().map(|p| p.height));
        coordinates = match height {
            Some(y) => format!(
                "{}: X {}  Y {}{}  Z {}",
                frame.view.dimension.label(),
                block[0],
                if height_approximate { "≈" } else { "" },
                y,
                block[1]
            ),
            None => format!(
                "{}: X {}  Z {}",
                frame.view.dimension.label(),
                block[0],
                block[1]
            ),
        };
        if let Some([x, _, z]) = runtime::portal_coordinates(
            frame.view.dimension,
            Dimension::Nether,
            [f64::from(block[0]), 0.0, f64::from(block[1])],
        ) {
            nether = localized_format!(
                "Nether (cursor): X {x}  Z {z}",
                "Незер (точка под мышью): X {x}  Z {z}",
                "Nether (curseur) : X {x}  Z {z}",
                "Nether (cursor): X {x}  Z {z}",
                "Nether (Mauszeiger): X {x}  Z {z}"
            );
        }
        if state.dimension == frame.view.dimension {
            if let Some(y) = height.or_else(|| game.native_player().map(|p| p.y.floor() as i32)) {
                biome = known_biome(game, state.dimension, [block[0], y, block[1]]);
            }
        }
        if biome.is_none() {
            biome = rough
                .as_ref()
                .and_then(|sample| sample.biome.as_ref())
                .map(|name| name.to_string());
        }
        lod_warning = !coverage.detailed_at(frame.view.dimension, frame.view.layer, block)
            && coverage
                .lod_at(frame.view.dimension, frame.view.layer, block)
                .is_some();
        if let Some(waypoint) = waypoint_at_pointer(state, &frame.view, rect, mouse) {
            tooltip = Some(format!(
                "{}  ({} {} {})",
                waypoint.name, waypoint.x, waypoint.y, waypoint.z
            ));
        }
    }
    let settings = runtime::initialize(game.native_data_dir(runtime::MOD_ID)).settings();
    for (i, dimension) in Dimension::ALL.into_iter().enumerate() {
        let tab = tab(batch.width, i);
        button(game, batch, tab, "", mouse, true);
        if dimension == settings.world_dimension {
            game.native_fill(batch, tab, 0xe032624b);
        }
        dimension_icon(game, batch, dimension, [tab[0] + 4.0, tab[1] + 5.0]);
        game.native_text(
            batch,
            dimension.label(),
            [tab[0] + 16.0, tab[1] + 5.0],
            0xffffff,
            true,
        );
    }
    let footer = map_footer(batch.width, batch.height);
    game.native_fill(batch, footer, 0xdf10151d);
    game.native_text(
        batch,
        &coordinates,
        [footer[0] + 6.0, footer[1] + 5.0],
        0xffffff,
        true,
    );
    dimension_icon(
        game,
        batch,
        Dimension::Nether,
        [footer[0] + 6.0, footer[1] + 18.0],
    );
    game.native_text(
        batch,
        &nether,
        [footer[0] + 18.0, footer[1] + 18.0],
        0xffffff,
        true,
    );
    biome_icon(
        game,
        batch,
        biome.as_deref(),
        [footer[0] + 6.0, footer[1] + 31.0],
    );
    let mut label = biome.as_deref().map_or_else(
        || tr("Biome: —", "биом: —", "Biome : —", "Bioma: —", "Biom: —").to_owned(),
        localized_biome,
    );
    if lod_warning {
        label.push_str(tr(
            " | LOD — approximate",
            " | LOD — приблизительно",
            " | LOD — approximatif",
            " | LOD — aproximado",
            " | LOD — ungefähr",
        ));
    }
    game.native_text(
        batch,
        &label,
        [footer[0] + 18.0, footer[1] + 31.0],
        if lod_warning { 0xffffd45c } else { 0xffffff },
        true,
    );
    if let Some(tooltip) = tooltip
        && state.info.is_none()
    {
        draw_tooltip(game, batch, &tooltip, mouse);
    }
    draw_menu(game, batch, state);
    draw_notice(game, batch, state);
    if let Some(info) = state.info {
        draw_information(game, batch, state, info);
    }
}

// Only the map and its name prompt cover the world. Other screens keep
// Horizon's normal rendering and overlay priorities.
fn map_covers_world(state: &State) -> bool {
    state.mode == Mode::Map
        || (state.mode == Mode::Name
            && state
                .name_form
                .as_ref()
                .is_some_and(|form| form.background == Mode::Map))
}

pub(super) fn transparent_waypoints(game: &Game<'_>) -> bool {
    if !game.native_on(runtime::MOD_ID) || game.native_mod_screen() != Some(runtime::MOD_ID) {
        return false;
    }
    let state = state();
    state.mode == Mode::Waypoints
        || (state.mode == Mode::Name
            && state
                .name_form
                .as_ref()
                .is_some_and(|form| form.background == Mode::Waypoints))
}

pub(super) fn covers_world(game: &Game<'_>) -> bool {
    game.native_on(runtime::MOD_ID)
        && game.native_mod_screen() == Some(runtime::MOD_ID)
        && game.native_player().is_some()
        && map_covers_world(&state())
}

pub(super) fn screen(game: &Game<'_>, batch: &mut HudBatch) {
    if game.native_mod_screen() != Some(runtime::MOD_ID) || !game.native_on(runtime::MOD_ID) {
        return;
    }
    let state = state();
    match state.mode {
        Mode::Map => draw_map_screen(game, batch, &state),
        Mode::Waypoints => draw_waypoints_screen(game, batch, &state),
        Mode::Name => {
            if let Some(form) = state.name_form.as_ref() {
                draw_name_form(game, batch, &state, form);
            }
        }
    }
}

pub(super) fn press(
    game: &Game<'_>,
    screen: &crate::mod_api::NativeScreenView<'_>,
    button: i32,
    canceled: &mut bool,
) {
    if game.native_mod_screen() != Some(runtime::MOD_ID) || !game.native_on(runtime::MOD_ID) {
        return;
    }
    let chosen = game.native_mod_key(
        runtime::MOD_ID,
        runtime::WORLD_MAP_KEY,
        runtime::WORLD_MAP_DEFAULT,
    );
    let mouse = match button {
        0 => "key.mouse.left",
        1 => "key.mouse.right",
        2 => "key.mouse.middle",
        _ => "",
    };
    if chosen == mouse {
        state().close = true;
        *canceled = true;
        return;
    }
    let mut state = state();
    if state.info.is_some() {
        if button == 0 {
            let rect = information_rect(screen.rect[2], screen.rect[3]);
            let close = [
                rect[0] + rect[2] - 82.0,
                rect[1] + rect[3] - 24.0,
                72.0,
                18.0,
            ];
            if !inside(screen.mouse, rect) || inside(screen.mouse, close) {
                state.info = None;
            }
        }
        *canceled = true;
        return;
    }
    match state.mode {
        Mode::Map => map_press(game, &mut state, screen, button, canceled),
        Mode::Waypoints => waypoint_press(game, &mut state, screen, button, canceled),
        Mode::Name => name_press(game, &mut state, screen, button, canceled),
    }
}

fn map_press(
    game: &Game<'_>,
    state: &mut State,
    screen: &crate::mod_api::NativeScreenView<'_>,
    button: i32,
    canceled: &mut bool,
) {
    if state.menu.is_none() && over_map_controls(screen.mouse, screen.rect[2], screen.rect[3]) {
        if button == 0 {
            for (i, dimension) in Dimension::ALL.into_iter().enumerate() {
                if inside(screen.mouse, tab(screen.rect[2], i)) {
                    runtime::initialize(game.native_data_dir(runtime::MOD_ID))
                        .choose_dimension(true, dimension);
                    break;
                }
            }
        }
        state.menu = None;
        *canceled = true;
        return;
    }
    if button == 1 {
        let menu = menu_target_at(game, state, screen, screen.mouse);
        state.menu = menu;
        *canceled = true;
        return;
    }
    if button != 0 {
        return;
    }
    let width = screen.rect[2];
    state.dragging = false;
    if let Some(menu) = state.menu {
        let Some(player) = game.native_player() else {
            state.menu = None;
            return;
        };
        let subtitle = menu_subtitle(state, menu, &player);
        let target = portal_target(state, menu, player.y.floor() as i32);
        let items = menu_items(game, menu, target);
        let (rect, rows) = menu_layout(
            game,
            [screen.rect[2], screen.rect[3]],
            menu,
            subtitle.as_deref(),
            &items,
        );
        if inside(screen.mouse, rect) {
            for ((_, action), row) in items.iter().zip(rows) {
                if inside(screen.mouse, row) {
                    perform_menu(game, state, menu, *action);
                    *canceled = true;
                    return;
                }
            }
            *canceled = true;
            return;
        }
        state.menu = None;
    }
    let portal_click = state
        .frame
        .as_ref()
        .filter(|frame| {
            frame.view.dimension == state.selected
                && frame.view.large
                && frame.view.session == state.session
        })
        .and_then(|frame| {
            hovered_block(&frame.view, rectangle(width, screen.rect[3]), screen.mouse)
        })
        .is_some_and(|block| portal_near(state, state.selected, block).is_some());
    if portal_click {
        let menu = menu_target_at(game, state, screen, screen.mouse);
        state.menu = menu;
        *canceled = true;
        return;
    }
    for (i, dimension) in Dimension::ALL.into_iter().enumerate() {
        if inside(screen.mouse, tab(width, i)) {
            runtime::initialize(game.native_data_dir(runtime::MOD_ID))
                .choose_dimension(true, dimension);
            *canceled = true;
            return;
        }
    }
    if inside(screen.mouse, rectangle(width, screen.rect[3])) {
        state.dragging = true;
        *canceled = true;
    }
}

fn menu_target_at(
    game: &Game<'_>,
    state: &State,
    screen: &crate::mod_api::NativeScreenView<'_>,
    at: [f32; 2],
) -> Option<Menu> {
    let rect = rectangle(screen.rect[2], screen.rect[3]);
    let frame = state.frame.as_ref().filter(|frame| {
        frame.view.dimension == state.selected
            && frame.view.large
            && frame.view.session == state.session
    })?;
    let block = hovered_block(&frame.view, rect, at)?;
    let portal = portal_near(state, state.selected, block);
    let height = if let Some((_, at)) = portal {
        Some(at[1])
    } else if state.dimension == frame.view.dimension {
        hovered_height(game, frame.view.layer, block)
    } else {
        match frame.view.layer {
            Layer::Below(y) => Some(y),
            Layer::Surface => None,
        }
    };
    let rough = runtime::initialize(game.native_data_dir(runtime::MOD_ID))
        .maps
        .coverage()
        .lod_sample(state.selected, frame.view.layer, block);
    let height_approximate = height.is_none() && rough.is_some();
    let height = height.or_else(|| rough.map(|p| p.height));
    Some(Menu {
        height_approximate,
        block,
        height,
        dimension: state.selected,
        at,
        portal: portal.map(|(kind, _)| kind),
        waypoint: state
            .waypoints
            .iter()
            .find(|waypoint| {
                waypoint.dimension == state.selected
                    && waypoint.x == block[0]
                    && waypoint.z == block[1]
            })
            .map(|waypoint| Spot {
                dimension: waypoint.dimension,
                x: waypoint.x,
                z: waypoint.z,
            }),
    })
}

fn waypoint_press(
    game: &Game<'_>,
    state: &mut State,
    screen: &crate::mod_api::NativeScreenView<'_>,
    button: i32,
    canceled: &mut bool,
) {
    if button != 0 {
        return;
    }
    let create = [screen.rect[2] - 260.0, 6.0, 120.0, 18.0];
    let back = [screen.rect[2] - 132.0, 6.0, 120.0, 18.0];
    if inside(screen.mouse, create) {
        let Some(player) = game.native_player() else {
            return;
        };
        state.name_form = Some(NameForm {
            text: default_waypoint_name(state),
            target: Some(Target {
                dimension: Dimension::from_id(player.dimension),
                x: player.x.floor() as i32,
                y: player.y.floor() as i32,
                z: player.z.floor() as i32,
            }),
            rename: None,
            background: Mode::Waypoints,
        });
        state.mode = Mode::Name;
        runtime::set_text_input(true);
        *canceled = true;
        return;
    }
    if inside(screen.mouse, back) {
        state.mode = Mode::Map;
        *canceled = true;
        return;
    }
    for index in 0..state.waypoints.len() {
        if let Some(row) = waypoint_row_at(state, screen.rect[2], screen.rect[3], index)
            && inside(screen.mouse, row)
        {
            state.waypoint_selected = Some(index);
            *canceled = true;
            return;
        }
    }
    let [rename, delete, show] = waypoint_actions(screen.rect[2], screen.rect[3]);
    if inside(screen.mouse, show) {
        if let Some(dimension) = show_selected_waypoint(state) {
            runtime::initialize(game.native_data_dir(runtime::MOD_ID))
                .choose_dimension(true, dimension);
        }
        *canceled = true;
        return;
    }
    if inside(screen.mouse, rename)
        && let Some(index) = state.waypoint_selected
        && let Some(waypoint) = state.waypoints.get(index)
    {
        state.name_form = Some(NameForm {
            text: waypoint.name.clone(),
            target: None,
            rename: Some(Spot {
                dimension: waypoint.dimension,
                x: waypoint.x,
                z: waypoint.z,
            }),
            background: Mode::Waypoints,
        });
        state.mode = Mode::Name;
        runtime::set_text_input(true);
        *canceled = true;
        return;
    }
    if inside(screen.mouse, delete)
        && let Some(index) = state.waypoint_selected
        && let Some(waypoint) = state.waypoints.get(index).cloned()
    {
        runtime::initialize(game.native_data_dir(runtime::MOD_ID)).remove_waypoint(
            &state.world,
            waypoint.dimension,
            waypoint.x,
            waypoint.z,
        );
        state.notice = Some((
            tr(
                "Waypoint deleted",
                "Вейпоинт удалён",
                "Repère supprimé",
                "Marcador eliminado",
                "Wegpunkt gelöscht",
            )
            .into(),
            Instant::now(),
        ));
        state.waypoint_selected = None;
        *canceled = true;
    }
}

fn name_press(
    game: &Game<'_>,
    state: &mut State,
    screen: &crate::mod_api::NativeScreenView<'_>,
    button: i32,
    canceled: &mut bool,
) {
    if button != 0 {
        return;
    }
    let width = 340.0_f32.min(screen.rect[2] - 24.0);
    let rect = [
        (screen.rect[2] - width) / 2.0,
        screen.rect[3] / 2.0 - 45.0,
        width,
        90.0,
    ];
    let save = [
        rect[0] + rect[2] - 184.0,
        rect[1] + rect[3] - 24.0,
        80.0,
        18.0,
    ];
    let cancel = [
        rect[0] + rect[2] - 96.0,
        rect[1] + rect[3] - 24.0,
        80.0,
        18.0,
    ];
    if inside(screen.mouse, save) {
        save_name(game, state);
        *canceled = true;
    } else if inside(screen.mouse, cancel) {
        cancel_name(state);
        *canceled = true;
    }
}

fn save_name(game: &Game<'_>, state: &mut State) {
    let Some(form) = state.name_form.take() else {
        return;
    };
    let controller = runtime::initialize(game.native_data_dir(runtime::MOD_ID));
    if let Some(rename) = form.rename {
        controller.rename_waypoint(
            &state.world,
            rename.dimension,
            rename.x,
            rename.z,
            &form.text,
        );
        state.notice = Some((
            tr(
                "Waypoint renamed",
                "Имя вейпоинта изменено",
                "Repère renommé",
                "Nombre del marcador cambiado",
                "Wegpunkt umbenannt",
            )
            .into(),
            Instant::now(),
        ));
    } else if let Some(target) = form.target {
        let saved = controller.add_waypoint(Waypoint {
            world: state.world.clone(),
            dimension: target.dimension,
            x: target.x,
            y: target.y,
            z: target.z,
            name: Waypoint::clean_name(&form.text),
        });
        state.notice = Some((
            if saved {
                tr(
                    "Waypoint saved",
                    "Вейпоинт сохранён",
                    "Repère enregistré",
                    "Marcador guardado",
                    "Wegpunkt gespeichert",
                )
            } else {
                tr(
                    "Waypoint unchanged: duplicate, limit reached or world unavailable",
                    "Вейпоинт не изменён: дубликат, лимит или мир недоступен",
                    "Repère inchangé : doublon, limite atteinte ou monde indisponible",
                    "Marcador sin cambios: duplicado, límite alcanzado o mundo no disponible",
                    "Wegpunkt unverändert: Duplikat, Grenze erreicht oder Welt nicht verfügbar",
                )
            }
            .into(),
            Instant::now(),
        ));
    }
    state.mode = form.background;
    runtime::set_text_input(false);
}

fn cancel_name(state: &mut State) {
    if let Some(form) = state.name_form.take() {
        state.mode = form.background;
    }
    runtime::set_text_input(false);
}

pub(super) fn drag(game: &Game<'_>, delta: [f32; 2], button: i32, canceled: &mut bool) {
    if game.native_mod_screen() != Some(runtime::MOD_ID)
        || !game.native_on(runtime::MOD_ID)
        || button != 0
    {
        return;
    }
    let mut state = state();
    if state.info.is_some() {
        *canceled = true;
        return;
    }
    if state.mode != Mode::Map || !state.dragging {
        return;
    }
    state.camera.pan([f64::from(delta[0]), f64::from(delta[1])]);
    state.elapsed = 1.0;
    *canceled = true;
}

pub(super) fn scroll(
    game: &Game<'_>,
    screen: &crate::mod_api::NativeScreenView<'_>,
    scroll: [f64; 2],
    canceled: &mut bool,
) {
    if game.native_mod_screen() != Some(runtime::MOD_ID) || !game.native_on(runtime::MOD_ID) {
        return;
    }
    let mut state = state();
    if state.info.is_some() {
        let rect = information_rect(screen.rect[2], screen.rect[3]);
        let visible = ((rect[3] - 56.0) / 12.0).max(1.0) as usize;
        let lines = information_lines(game, &state, state.info.unwrap(), rect[2] - 24.0);
        let max = lines.len().saturating_sub(visible);
        state.info_scroll =
            (state.info_scroll.min(max) as i32 - scroll[1] as i32).clamp(0, max as i32) as usize;
        *canceled = true;
        return;
    }
    if state.mode == Mode::Waypoints {
        let rows = state
            .waypoints
            .len()
            .saturating_sub(waypoint_visible_rows(screen.rect[3]));
        let next = (state.waypoint_scroll as i32 - scroll[1] as i32).clamp(0, rows as i32);
        state.waypoint_scroll = next as usize;
        *canceled = true;
        return;
    }
    if state.mode != Mode::Map {
        return;
    }
    if over_map_controls(screen.mouse, screen.rect[2], screen.rect[3]) {
        *canceled = true;
        return;
    }
    let rect = rectangle(screen.rect[2], screen.rect[3]);
    if !inside(screen.mouse, rect) {
        return;
    }
    let offset = [
        f64::from(screen.mouse[0] - rect[0] - rect[2] / 2.0),
        f64::from(screen.mouse[1] - rect[1] - rect[3] / 2.0),
    ];
    state.camera.zoom_at(scroll[1], offset);
    state.elapsed = 1.0;
    *canceled = true;
}

pub(super) fn screen_key(game: &Game<'_>, key: &str, canceled: &mut bool) {
    if game.native_mod_screen() != Some(runtime::MOD_ID) || !game.native_on(runtime::MOD_ID) {
        return;
    }
    let mut state = state();
    if state.info.is_some() {
        if key == "key.keyboard.escape" {
            state.info = None;
        }
        *canceled = true;
        return;
    }
    if state.mode == Mode::Name {
        match key {
            "key.keyboard.escape" => {
                cancel_name(&mut state);
                *canceled = true;
            }
            "key.keyboard.backspace" => {
                if let Some(form) = state.name_form.as_mut() {
                    form.text.pop();
                }
                *canceled = true;
            }
            "key.keyboard.enter" | "key.keyboard.keypad.enter" => {
                save_name(game, &mut state);
                *canceled = true;
            }
            _ => {
                *canceled = true;
            }
        }
        return;
    }
    if key == "key.keyboard.escape" {
        runtime::set_text_input(false);
        state.close = true;
        *canceled = true;
    }
}

pub(super) fn screen_char(game: &Game<'_>, c: char, canceled: &mut bool) {
    if game.native_mod_screen() != Some(runtime::MOD_ID) || !game.native_on(runtime::MOD_ID) {
        return;
    }
    let mut state = state();
    if state.mode != Mode::Name {
        return;
    }
    if c.is_control() {
        return;
    }
    if let Some(form) = state.name_form.as_mut()
        && form.text.chars().count() < 32
    {
        form.text.push(c);
    }
    *canceled = true;
}

pub(super) fn disconnect(game: &Game<'_>) {
    if !game.native_window() {
        return;
    }
    runtime::set_text_input(false);
    let mut s = state();
    let next = s.session + 1;
    *s = State {
        session: next,
        ..State::default()
    };
    runtime::initialize(game.native_data_dir(runtime::MOD_ID))
        .maps
        .reset(next);
}

pub(super) fn atlas_invalidated() {
    state().uploaded = (0, 0);
}

#[cfg(test)]
mod render_tests {
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
    fn map_camera_never_advances_beyond_the_uploaded_atlas() {
        let atlas = maps::TerrainAtlas {
            serial: 1,
            pixels: Vec::new(),
            width: 1024,
            height: 16,
            chunks: Vec::new(),
            coarse: Vec::new(),
            region: [-128, -128, 256, 256],
            lod: 0,
        };
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
            smooth: true,
        };
        let mut other = prepared.clone();
        other.dimension = Dimension::Nether;
        assert_eq!(complete_map_view(&atlas, other, None, &prepared), prepared);
        let mut previous = prepared.clone();
        previous.center = [16.0, 0.0];
        let mut desired = prepared.clone();
        desired.span = [1024.0; 2];
        assert_eq!(
            complete_map_view(&atlas, desired.clone(), Some(&previous), &prepared),
            previous
        );
        previous.session = 2;
        assert_eq!(
            complete_map_view(&atlas, desired, Some(&previous), &prepared),
            prepared
        );
        let mut desired = prepared.clone();
        desired.center = [12.0, 0.0];
        assert_eq!(
            complete_map_view(&atlas, desired.clone(), None, &prepared),
            desired
        );
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
