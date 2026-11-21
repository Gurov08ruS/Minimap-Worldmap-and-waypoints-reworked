// SPDX-License-Identifier: GPL-3.0-only
// Engine adapter: this module is injected under game, never into the standalone library.
use super::Game;
use super::minimap_compat::MapAccess;
use runtime::{Language, language, tr};

macro_rules! localized_format {
    ($en:literal, $ru:literal, $fr:literal, $es:literal, $de:literal, $zh:literal $(, $arg:expr)* $(,)?) => {
        match language() {
            Language::Russian => format!($ru $(, $arg)*),
            Language::French => format!($fr $(, $arg)*),
            Language::Spanish => format!($es $(, $arg)*),
            Language::German => format!($de $(, $arg)*),
            Language::SimplifiedChinese => format!($zh $(, $arg)*),
            Language::English => format!($en $(, $arg)*),
        }
    };
}

use ::minimap_worldmap_waypoints_reworked::runtime::{
    self as runtime, Dimension, Waypoint,
    maps::{self, Layer, Portal, View},
};
use horizon_client::{
    chunk::Chunk, client_resources::ClientResources, extras::Corner, extras_hud::Corners,
    hud::HudBatch, mesher::TintSource,
};
use horizon_core::geometry::Direction;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, OnceLock, Weak},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Map,
    Waypoints,
    Name,
    Caches,
}

#[derive(Default)]
struct CacheUi {
    title_open: bool,
    pad_buttons: u8,
    preview: Option<CachePreview>,
    confirm_delete: Option<runtime::container::DeleteSelection>,
    deleting: bool,
    attention: bool,
    catalog_revision: u64,
    selected: Option<String>,
    scroll: runtime::ListScroll,
    scrollbar_grab: Option<f32>,
    revision: u64,
    icons: BTreeMap<String, Arc<horizon_client::hud::pictures::Picture>>,
    operation: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    message: Option<String>,
    confirm: Option<String>,
}

type PageImages = Vec<(
    Arc<maps::AtlasPage>,
    Arc<horizon_client::hud::pictures::Picture>,
)>;
#[derive(Clone, PartialEq)]
struct PreviewRequest {
    view: View,
    target: View,
    point: Option<[i32; 2]>,
}

#[derive(Clone)]
struct PreparedPreview {
    dimension: Dimension,
    layer: Layer,
    atlas: Arc<maps::TerrainAtlas>,
    point: Option<[i32; 2]>,
    column: Option<maps::CachedColumn>,
    portals: Arc<Vec<([i32; 3], Portal)>>,
}

struct CachePreview {
    name: String,
    world: String,
    menu: Option<Menu>,
    form: Option<PreviewForm>,
    notice: Option<(String, Instant)>,
    waypoints: Vec<Waypoint>,
    waypoint_labels: Vec<String>,
    waypoint_letters: runtime::minimap::WaypointLetters,
    wake: std::sync::mpsc::SyncSender<()>,
    request: Arc<Mutex<Option<PreviewRequest>>>,
    prepared: Arc<Mutex<Option<PreparedPreview>>>,
    response: std::sync::mpsc::Receiver<PreviewUpdate>,
    dimensions: Vec<maps::PreviewDimension>,
    layer_index: usize,
    position: Option<maps::LastPosition>,
    camera: maps::Camera,
    last_frame: Instant,
    dimension: Dimension,
    uploaded: Option<Arc<maps::TerrainAtlas>>,
    page_images: PageImages,
    dragging: bool,
    error: Option<String>,
}

enum PreviewForm {
    Waypoint(NameForm),
    Coordinates { text: String, invalid: bool },
}

enum PreviewUpdate {
    Ready(Vec<maps::PreviewDimension>, Option<maps::LastPosition>),
    Error(String),
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
    color: u32,
    target: Option<Target>,
    rename: Option<Spot>,
    background: Mode,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FluidKind {
    Water,
    Lava,
}

#[derive(Clone, Copy)]
struct FluidColumn {
    kind: FluidKind,
    bottom: i32,
}

#[derive(Clone, Copy)]
struct ColumnInfo {
    height: i32,
    fluid: Option<FluidColumn>,
}

#[derive(Clone, Copy)]
struct Menu {
    height_approximate: bool,
    block: [i32; 2],
    height: Option<i32>,
    fluid: Option<FluidColumn>,
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
    EditWaypoint,
    TeleportWaypoint,
    MoveView,
    Toggle3D,
    Information,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LeafTint {
    None,
    Fixed(u32),
    Grass,
    Foliage,
}

#[derive(Clone, Copy)]
struct LeafColor {
    /// Mean colour of the leaf texture, without its tint.
    rgb: u32,
    tint: LeafTint,
}

impl LeafColor {
    fn map_color(self, grass: Option<u32>, foliage: Option<u32>) -> Option<u32> {
        let tint = match self.tint {
            LeafTint::None => 0x00ff_ffff,
            LeafTint::Fixed(color) => color,
            LeafTint::Grass => grass?,
            LeafTint::Foliage => foliage?,
        };
        Some(multiply_color(self.rgb, tint))
    }
}

fn multiply_color(base: u32, tint: u32) -> u32 {
    let channel = |shift: u32| ((((base >> shift) & 255) * ((tint >> shift) & 255)) / 255) & 255;
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

struct LeafPalette {
    /// Block state id → the texture colour the game draws with, before tint.
    states: BTreeMap<u32, LeafColor>,
    snow: u32,
    ice: u32,
}
impl Default for LeafPalette {
    fn default() -> Self {
        Self {
            states: BTreeMap::new(),
            snow: 0xffffff,
            ice: 0xa0a0ff,
        }
    }
}

fn average_texture(resources: &ClientResources, name: &str) -> Option<u32> {
    let pixels = resources.texture(name)?;
    let mut sum = [0u64; 3];
    let mut weight = 0u64;
    for pixel in pixels.rgba.chunks_exact(4) {
        let alpha = u64::from(pixel[3]);
        if alpha == 0 {
            continue;
        }
        for (total, value) in sum.iter_mut().zip(&pixel[..3]) {
            *total += u64::from(*value) * alpha;
        }
        weight += alpha;
    }
    if weight == 0 {
        return None;
    }
    let red = (sum[0] / weight) as u32;
    let green = (sum[1] / weight) as u32;
    let blue = (sum[2] / weight) as u32;
    Some((red << 16) | (green << 8) | blue)
}

/// Mean texture colour and the game's tint source for every leaf block,
/// including vines and lily pads, whose tint the map takes from the biome.
/// Resource packs move the textures, so the result follows
/// `block_sprites_version`.
fn build_leaf_palette(game: &Game<'_>) -> LeafPalette {
    let names =
        horizon_client::block_sprites::texture_names(&game.pack.context.render.sprite_layers);
    let states = horizon_client::block_sprites::Lookup::new(
        &game.pack.context,
        game.renderer
            .as_ref()
            .map_or(u32::MAX, |r| r.sprite_split()),
        &names,
    );
    let resources = ClientResources::new(game.options.resources.clone());
    let book = game.res.facts();
    let mut textures = BTreeMap::<String, Option<u32>>::new();
    let mut palette = LeafPalette::default();
    for (index, facts) in book.states.iter().enumerate() {
        let id = horizon_world::registry::BlockStateId(index as u32);
        let name = facts.name.as_ref();
        if let Some(rgb) = book
            .states
            .get(id.0 as usize)
            .and_then(|state| horizon_client::blocks::MAP_COLORS.get(state.map_color as usize))
        {
            if name == "minecraft:snow_block" {
                palette.snow = *rgb;
            }
            if name == "minecraft:ice" {
                palette.ice = *rgb;
            }
        }
        let tinted_block = name.contains("leaves")
            || matches!(name, "minecraft:vine" | "minecraft:lily_pad")
            || book.states.get(id.0 as usize).is_some_and(|state| {
                state
                    .classes
                    .iter()
                    .any(|class| class.as_ref() == "LeavesBlock")
            });
        if !tinted_block {
            continue;
        }
        let sprites = states.state(id);
        let face = sprites.sides[Direction::Up as usize]
            .first()
            .or_else(|| sprites.sides.iter().find_map(|side| side.first()));
        let Some(face) = face else {
            continue;
        };
        let texture = face.texture.as_str();
        let rgb = *textures
            .entry(texture.to_owned())
            .or_insert_with(|| average_texture(&resources, texture));
        let Some(rgb) = rgb else {
            continue;
        };
        let tint = if name == "minecraft:lily_pad" {
            // Vanilla tints a lily pad with the biome's grass colour; the
            // mesher's constant is only a fallback for its own drawing pass.
            LeafTint::Grass
        } else {
            match face.tint {
                None => LeafTint::None,
                Some(TintSource::Fixed(color)) => LeafTint::Fixed(color),
                Some(TintSource::Grass) => LeafTint::Grass,
                Some(TintSource::Foliage) => LeafTint::Foliage,
                // Dry foliage and water belong to non-leaf blocks; keep their
                // map colour instead of pretending a wrong tint.
                Some(_) => continue,
            }
        };
        palette.states.insert(id.0, LeafColor { rgb, tint });
    }
    palette
}

#[derive(Clone, Copy, Default)]
struct BiomeTints {
    grass: Option<u32>,
    foliage: Option<u32>,
    water: Option<u32>,
}

// Minecraft blends the biome colours over a radius instead of switching at a
// line. Sample a small grid around the block and average each requested tint.
const BIOME_BLEND_RADIUS: i32 = 4;
const BIOME_BLEND_STEP: i32 = 2;

fn blend_biome_tints(
    palette: &horizon_client::biome::Biomes,
    at: [i32; 2],
    radius: i32,
    step: i32,
    grass: bool,
    foliage: bool,
    water: bool,
    mut biome: impl FnMut(i32, i32) -> Option<u16>,
) -> BiomeTints {
    if !(grass || foliage || water) {
        return BiomeTints::default();
    }
    let step = step.max(1);
    let mut sums = [[0u64; 3]; 3];
    let mut counts = [0u64; 3];
    let mut dz = -radius;
    while dz <= radius {
        let mut dx = -radius;
        while dx <= radius {
            let x = at[0].saturating_add(dx);
            let z = at[1].saturating_add(dz);
            if let Some(id) = biome(x, z)
                && let Some(info) = palette.list.get(usize::from(id))
            {
                if grass {
                    add_color(
                        &mut sums[0],
                        &mut counts[0],
                        palette.grass(id, f64::from(x), f64::from(z)),
                    );
                }
                if foliage {
                    add_color(&mut sums[1], &mut counts[1], info.foliage);
                }
                if water {
                    add_color(&mut sums[2], &mut counts[2], info.water);
                }
            }
            dx += step;
        }
        dz += step;
    }
    BiomeTints {
        grass: average_color(sums[0], counts[0]),
        foliage: average_color(sums[1], counts[1]),
        water: average_color(sums[2], counts[2]),
    }
}

fn add_color(sum: &mut [u64; 3], count: &mut u64, color: u32) {
    for (channel, shift) in sum.iter_mut().zip([16, 8, 0]) {
        *channel += u64::from((color >> shift) & 255);
    }
    *count += 1;
}

fn average_color(sum: [u64; 3], count: u64) -> Option<u32> {
    if count == 0 {
        return None;
    }
    let channel = |index: usize| (sum[index] / count) as u32;
    Some((channel(0) << 16) | (channel(1) << 8) | channel(2))
}

struct State {
    caches: CacheUi,
    world: String,
    session: u64,
    dimension: Dimension,
    seen: BTreeMap<(i32, i32), (Weak<Chunk>, Layer, Instant)>,
    chunk_keys: BTreeSet<(i32, i32)>,
    chunk_revision: u64,
    chunk_cursor: Option<(i32, i32)>,
    layers: BTreeMap<Dimension, Layer>,
    selected: Dimension,
    large: bool,
    camera: maps::Camera,
    marker: [f64; 2],
    elapsed: f64,
    frame: Option<Arc<maps::Frame>>,
    page_images: PageImages,
    dragging: bool,
    close: bool,
    mode: Mode,
    menu: Option<Menu>,
    name_form: Option<NameForm>,
    waypoints: Vec<Waypoint>,
    waypoint_label_source: Vec<Waypoint>,
    waypoint_labels: Vec<String>,
    waypoint_letter_mode: runtime::minimap::WaypointLetters,
    portals: Vec<(Dimension, [i32; 3], Portal)>,
    waypoint_scroll: runtime::ListScroll,
    waypoint_selected: Option<usize>,
    notice: Option<(String, Instant)>,
    pending_copy: Option<String>,
    pending_command: Option<String>,
    map_focus: Option<Spot>,
    /// Chunks the server sent since the last sampling pass. A waypoint's biome
    /// is sampled when its chunk arrives, rather than polling every point.
    biome_chunks: BTreeSet<(i32, i32)>,
    leaves: Option<(u64, Arc<LeafPalette>)>,
    lod_elapsed: f64,
    lod_sources:
        BTreeMap<(Dimension, maps::LodKey), (Weak<Vec<horizon_client::lod::Column>>, Instant)>,
    info: Option<Menu>,
    info_scroll: runtime::ListScroll,
}
fn known_lod_sources(state: &State, dimension: Dimension) -> BTreeMap<(i32, i32, u8), usize> {
    state
        .lod_sources
        .iter()
        .filter(|((d, _), (source, _))| *d == dimension && source.strong_count() != 0)
        .map(|((_, key), (source, _))| ((key.x, key.z, key.level), source.as_ptr() as usize))
        .collect()
}
impl Default for State {
    fn default() -> Self {
        Self {
            caches: CacheUi::default(),
            world: String::new(),
            session: 0,
            dimension: Dimension::Player,
            seen: BTreeMap::new(),
            chunk_keys: BTreeSet::new(),
            chunk_revision: 0,
            chunk_cursor: None,
            layers: BTreeMap::new(),
            selected: Dimension::Player,
            large: false,
            camera: maps::Camera::new([0.0; 2], 2.0),
            marker: [0.0; 2],
            elapsed: 1.0,
            frame: None,
            page_images: Vec::new(),
            dragging: false,
            close: false,
            mode: Mode::Map,
            menu: None,
            name_form: None,
            waypoints: Vec::new(),
            waypoint_label_source: Vec::new(),
            waypoint_labels: Vec::new(),
            waypoint_letter_mode: runtime::minimap::WaypointLetters::default(),
            portals: Vec::new(),
            waypoint_scroll: runtime::ListScroll::default(),
            waypoint_selected: None,
            notice: None,
            pending_copy: None,
            pending_command: None,
            map_focus: None,
            biome_chunks: BTreeSet::new(),
            leaves: None,
            lod_elapsed: 1.0,
            lod_sources: BTreeMap::new(),
            info: None,
            info_scroll: runtime::ListScroll::default(),
        }
    }
}
impl State {
    fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        runtime::set_waypoint_background_transparent(
            mode == Mode::Waypoints
                || (mode == Mode::Name
                    && self
                        .name_form
                        .as_ref()
                        .is_some_and(|form| form.background == Mode::Waypoints)),
        );
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
        "cave_vines" | "cave_vines_plant" => 0x3f6f2f,
        "twisting_vines" | "twisting_vines_plant" => 0x3fa0a0,
        "weeping_vines" | "weeping_vines_plant" => 0x8a3030,
        "sugar_cane" => 0x8fbf5a,
        "bamboo" => 0xb7c65a,
        "cactus" => 0x3f7f3f,
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
    rows: [&str; 16],
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
                "................",
                ".......11.......",
                "......1111......",
                ".....111111.....",
                "....11211211....",
                "...1122112211...",
                "..112221122211..",
                ".11222211222211.",
                ".......11.......",
                ".......11.......",
                ".......11.......",
                ".......11.......",
                "......2112......",
                ".....222222.....",
                "................",
                "................",
            ],
            [0xffff6262, 0xffffffff, 0xffffffff],
        ),
        Dimension::Overworld => (
            [
                "................",
                "................",
                "..111111111111..",
                ".11111111111111.",
                ".11111111111111.",
                ".22111221112212.",
                ".22212222122222.",
                ".22222222222222.",
                ".22332222233222.",
                ".22222232222222.",
                ".22222222222222.",
                ".23222222222322.",
                "..222233222222..",
                "....22222222....",
                "......2222......",
                "................",
            ],
            [0xff68b34a, 0xff93643d, 0xffbc8a54],
        ),
        Dimension::Nether => (
            [
                "................",
                "..111111111111..",
                "..111111111111..",
                "..112222222211..",
                "..112233222211..",
                "..112333322211..",
                "..112233222211..",
                "..112222233211..",
                "..112222333211..",
                "..112222233211..",
                "..112332222211..",
                "..112233222211..",
                "..112222222211..",
                "..111111111111..",
                "..111111111111..",
                "................",
            ],
            [0xff44263f, 0xffb649cf, 0xffef9aff],
        ),
        Dimension::End => (
            [
                "................",
                "................",
                "......2222......",
                "....22111122....",
                "..221111111122..",
                ".22111333311122.",
                ".21113333331112.",
                ".21133333333112.",
                ".22113333331122.",
                "..221133331122..",
                "...2211111122...",
                "....22211222....",
                "......2222......",
                ".......22.......",
                "................",
                "................",
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
    game.map_biome(at)
}

fn localized_biome(id: &str) -> String {
    horizon_client::lang::get_or(&format!("biome.{}", id.replace(':', ".")), id)
}

fn biome_icon(game: &Game<'_>, batch: &mut HudBatch, biome: Option<&str>, at: [f32; 2]) {
    let icon = runtime::biome_pictogram(biome);
    for (y, row) in icon.rows.iter().enumerate() {
        let mut x = 0;
        while x < runtime::BIOME_ICON_SIZE {
            let index = (row >> (x * 2)) & 3;
            let start = x;
            while x < runtime::BIOME_ICON_SIZE && (row >> (x * 2)) & 3 == index {
                x += 1;
            }
            if index != 0 {
                game.native_fill(
                    batch,
                    [
                        at[0] + start as f32,
                        at[1] + y as f32,
                        (x - start) as f32,
                        1.0,
                    ],
                    icon.colors[index as usize - 1],
                );
            }
        }
    }
}

// A small skull for automatic death waypoints, drawn from relative blocks so
// it stays legible on the map, in the world and in the waypoint list.
fn draw_skull(game: &Game<'_>, batch: &mut HudBatch, at: [f32; 2], size: f32) {
    let u = size / 8.0;
    let bone = 0xffe9e6dc;
    let socket = 0xff14171c;
    game.native_fill(
        batch,
        [at[0] - 3.0 * u, at[1] - 3.0 * u, 6.0 * u, 5.0 * u],
        bone,
    );
    game.native_fill(
        batch,
        [at[0] - 2.0 * u, at[1] + 2.0 * u, 4.0 * u, 1.5 * u],
        bone,
    );
    for x in [-2.0, 0.75] {
        game.native_fill(
            batch,
            [at[0] + x * u, at[1] - 1.5 * u, 1.25 * u, 1.75 * u],
            socket,
        );
    }
    game.native_fill(
        batch,
        [at[0] - 0.5 * u, at[1] + 0.5 * u, 1.0 * u, 1.25 * u],
        socket,
    );
    for x in [-1.0, 0.5] {
        game.native_fill(
            batch,
            [at[0] + x * u, at[1] + 2.0 * u, 0.5 * u, 1.5 * u],
            socket,
        );
    }
}

fn portal_label(kind: Portal) -> &'static str {
    match kind {
        Portal::Nether => tr(
            "Nether portal",
            "Портал Незера",
            "Portail du Nether",
            "Portal del Nether",
            "Netherportal",
            "下界传送门",
        ),
        Portal::End => tr(
            "End portal",
            "Портал Энда",
            "Portail de l’End",
            "Portal del End",
            "Endportal",
            "末地传送门",
        ),
        Portal::Gateway => tr(
            "End gateway",
            "Врата Энда",
            "Passerelle de l’End",
            "Puerta de enlace del End",
            "Endtransitportal",
            "末地折跃门",
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

struct PlayerMarker {
    at: [f64; 2],
    yaw: Option<f64>,
    colors: (u32, u32),
}

fn draw_marker(
    game: &Game<'_>,
    batch: &mut HudBatch,
    view: &View,
    rect: [f32; 4],
    marker: PlayerMarker,
) {
    let pixel = view.screen_at(marker.at);
    let mut point = [
        rect[0] + pixel[0] as f32 * rect[2] / view.width as f32,
        rect[1] + pixel[1] as f32 * rect[3] / view.height as f32,
    ];
    let bounds = [rect[0] + 16., rect[1] + 42., rect[2] - 32., rect[3] - 100.];
    let (forward, size) = if view.large
        && let Some((edge, forward)) = maps::edge_indicator(point, bounds)
    {
        point = edge;
        (forward, 6.)
    } else if !inside(point, rect) {
        return;
    } else if let Some(yaw) = marker.yaw {
        (view.player_heading(yaw, [rect[2], rect[3]]), 7.)
    } else {
        game.native_fill(
            batch,
            [point[0] - 4., point[1] - 4., 8., 8.],
            marker.colors.0,
        );
        game.native_fill(
            batch,
            [point[0] - 2., point[1] - 2., 4., 4.],
            marker.colors.1,
        );
        return;
    };
    let [x, y] = point;
    let (border, fill) = marker.colors;
    let right = [-forward[1], forward[0]];
    for (size, color) in [(size, border), (size - 2., fill)] {
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
fn refresh_waypoint_labels(state: &mut State, mode: runtime::minimap::WaypointLetters) {
    if state.waypoint_label_source != state.waypoints || state.waypoint_letter_mode != mode {
        state.waypoint_labels = runtime::waypoint_labels(&state.waypoints, mode);
        state.waypoint_label_source.clone_from(&state.waypoints);
        state.waypoint_letter_mode = mode;
    }
}
fn waypoint_text<'a>(
    state: &'a State,
    index: usize,
    waypoint: &Waypoint,
) -> std::borrow::Cow<'a, str> {
    state.waypoint_labels.get(index).map_or_else(
        || std::borrow::Cow::Owned(first_letter(&waypoint.name).to_string()),
        |s| std::borrow::Cow::Borrowed(s.as_str()),
    )
}

fn waypoint_argb(color: u32) -> u32 {
    0xff00_0000 | (color & 0x00ff_ffff)
}

fn default_waypoint_color(state: &State) -> u32 {
    runtime::WAYPOINT_COLORS[state.waypoints.len() % runtime::WAYPOINT_COLORS.len()]
}

fn fit_text(game: &Game<'_>, text: &str, width: f32) -> String {
    if game.map_text_width(text) <= width {
        return text.to_owned();
    }
    let mut out = String::new();
    for c in text.chars() {
        let mut candidate = out.clone();
        candidate.push(c);
        candidate.push('…');
        if game.map_text_width(&candidate) > width {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

fn draw_waypoint(
    game: &Game<'_>,
    batch: &mut HudBatch,
    view: &View,
    rect: [f32; 4],
    waypoint: &Waypoint,
    letter: &str,
    size: f32,
) {
    let pixel = view.screen_at([f64::from(waypoint.x) + 0.5, f64::from(waypoint.z) + 0.5]);
    let x = rect[0] + pixel[0] as f32 * rect[2] / view.width as f32;
    let y = rect[1] + pixel[1] as f32 * rect[3] / view.height as f32;
    if !inside([x, y], rect) {
        return;
    }
    let half = size / 2.0;
    let width = if waypoint.icon == runtime::WaypointIcon::Letter {
        size.max(game.map_text_width(letter) + 4.)
    } else {
        size
    };
    game.native_fill(
        batch,
        [
            x - width / 2. - 1.0,
            y - half - 1.0,
            width + 2.0,
            size + 2.0,
        ],
        0xff101010,
    );
    game.native_fill(
        batch,
        [x - width / 2., y - half, width, size],
        waypoint_argb(waypoint.color),
    );
    if waypoint.icon == runtime::WaypointIcon::Skull {
        draw_skull(game, batch, [x, y], size * 0.95);
        return;
    }
    game.map_text(
        batch,
        &letter,
        [x - game.map_text_width(&letter) / 2.0, y - 4.0],
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
// (the only one whose loaded chunks the client holds). Water and lava also
// report the lowest block of their column.
fn hovered_column(game: &Game<'_>, layer: Layer, block: [i32; 2]) -> Option<ColumnInfo> {
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
    let height = chunk.highest(game.world.air.0, |y| {
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
            && (matches!(
                entry.fluid,
                horizon_client::blocks::Fluid::Water | horizon_client::blocks::Fluid::Lava
            ) || horizon_client::blocks::MAP_COLORS
                .get(entry.map_color as usize)
                .copied()
                .unwrap_or(0)
                != 0)
    })?;
    let fluid = facts
        .states
        .get(chunk.block(x, height, z).0 as usize)
        .and_then(|entry| match entry.fluid {
            horizon_client::blocks::Fluid::Water => Some(FluidKind::Water),
            horizon_client::blocks::Fluid::Lava => Some(FluidKind::Lava),
            horizon_client::blocks::Fluid::None | horizon_client::blocks::Fluid::Modded => None,
        })
        .map(|kind| {
            let mut bottom = height;
            while bottom > chunk.min_y() {
                let below = chunk.block(x, bottom - 1, z);
                let Some(entry) = facts.states.get(below.0 as usize) else {
                    break;
                };
                let same = match kind {
                    FluidKind::Water => entry.fluid == horizon_client::blocks::Fluid::Water,
                    FluidKind::Lava => entry.fluid == horizon_client::blocks::Fluid::Lava,
                };
                if !same {
                    break;
                }
                bottom -= 1;
            }
            FluidColumn { kind, bottom }
        });
    Some(ColumnInfo { height, fluid })
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
            "主世界：出生点",
        )
        .into(),
        PortalTarget::Outer => tr(
            "End: outer islands",
            "Энд: внешние острова",
            "End : îles extérieures",
            "End: islas exteriores",
            "Ende: äußere Inseln",
            "末地：外围岛屿",
        )
        .into(),
        PortalTarget::Unknown => tr(
            "unknown destination",
            "назначение неизвестно",
            "destination inconnue",
            "destino desconocido",
            "Unbekanntes Ziel",
            "未知目的地",
        )
        .into(),
    }
}

fn menu_title(menu: Menu) -> String {
    match menu.height {
        Some(y) => {
            let bottom = menu.fluid.map_or_else(String::new, |fluid| {
                localized_format!(
                    " (bottom {})",
                    " (дно {})",
                    " (fond {})",
                    " (fondo {})",
                    " (Grund {})",
                    "（底部 {}）",
                    fluid.bottom
                )
            });
            format!(
                "X: {}  Y: {}{}{}  Z: {}",
                menu.block[0],
                if menu.height_approximate { "≈" } else { "" },
                y,
                bottom,
                menu.block[1]
            )
        }
        None => format!("X: {}  Z: {}", menu.block[0], menu.block[1]),
    }
}

fn menu_subtitle(
    state: &State,
    menu: Menu,
    player: &crate::game::native_hooks::NativePlayer,
) -> Option<String> {
    if let Some(spot) = menu.waypoint {
        return state
            .waypoints
            .iter()
            .find(|w| waypoint_spot(w) == spot)
            .map(|w| w.name.clone());
    }
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
                    "Unbekanntes Ziel",
                    "未知目的地"
                )
                .to_owned(),
                portal_target_text
            )
        )
    })
}

fn waypoint_menu(can_teleport: bool) -> Vec<(&'static str, MenuAction)> {
    let mut items = vec![
        (
            tr(
                "Edit waypoint",
                "Изменить вейпоинт",
                "Modifier le repère",
                "Editar marcador",
                "Wegpunkt bearbeiten",
                "编辑路径点",
            ),
            MenuAction::EditWaypoint,
        ),
        (
            tr(
                "Delete waypoint",
                "Удалить вейпоинт",
                "Supprimer le repère",
                "Eliminar marcador",
                "Wegpunkt löschen",
                "删除路径点",
            ),
            MenuAction::DeleteWaypoint,
        ),
    ];
    if can_teleport {
        items.push((
            tr(
                "Teleport to waypoint",
                "Телепорт к вейпоинту",
                "Se téléporter au repère",
                "Teletransporte al marcador",
                "Zum Wegpunkt teleportieren",
                "传送到路径点",
            ),
            MenuAction::TeleportWaypoint,
        ));
    }
    items
}

fn move_view_label() -> &'static str {
    tr(
        "Move view to…",
        "Перенести вид на…",
        "Centrer la vue sur…",
        "Centrar vista en…",
        "Ansicht verschieben…",
        "移动视图到…",
    )
}

fn create_waypoint_label() -> &'static str {
    tr(
        "Create waypoint",
        "Поставить вейпоинт",
        "Créer un repère",
        "Crear marcador",
        "Wegpunkt erstellen",
        "创建路径点",
    )
}

fn menu_items(
    game: &Game<'_>,
    menu: Menu,
    target: Option<PortalTarget>,
    show_3d: bool,
) -> Vec<(&'static str, MenuAction)> {
    if menu.waypoint.is_some() {
        let mut items = waypoint_menu(teleport_allowed(game));
        items.push((
            if show_3d {
                tr(
                    "Hide in 3D world",
                    "Скрыть в 3D-мире",
                    "Masquer dans le monde 3D",
                    "Ocultar en el mundo 3D",
                    "In 3D-Welt ausblenden",
                    "在 3D 世界中隐藏",
                )
            } else {
                tr(
                    "Show in 3D world",
                    "Показать в 3D-мире",
                    "Afficher dans le monde 3D",
                    "Mostrar en el mundo 3D",
                    "In 3D-Welt zeigen",
                    "在 3D 世界中显示",
                )
            },
            MenuAction::Toggle3D,
        ));
        return items;
    }
    let mut items = vec![
        (
            tr(
                "Chunk and region information",
                "Информация о чанке и регионе",
                "Informations sur le chunk et la région",
                "Información del chunk y la región",
                "Chunk- und Regionsinformationen",
                "区块和区域信息",
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
                "复制坐标",
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
                "创建路径点",
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
                "复制目的地坐标",
            ),
            MenuAction::CopyDestination,
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
                        "传送到传送门",
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
                    "传送到这里",
                ),
                MenuAction::Teleport,
            ));
        }
    }
    items
}

/// Whether the waypoint under the pointer is shown in the 3D world; defaults to
/// true for a point no longer in the current snapshot.
fn waypoint_spot(waypoint: &Waypoint) -> Spot {
    Spot {
        dimension: waypoint.dimension,
        x: waypoint.x,
        z: waypoint.z,
    }
}

fn waypoint_form(waypoint: &Waypoint, background: Mode) -> NameForm {
    NameForm {
        text: waypoint.name.clone(),
        color: waypoint.color,
        target: None,
        rename: Some(waypoint_spot(waypoint)),
        background,
    }
}

fn waypoint_show_3d(state: &State, spot: Spot) -> bool {
    state
        .waypoints
        .iter()
        .find(|waypoint| {
            waypoint.dimension == spot.dimension && waypoint.x == spot.x && waypoint.z == spot.z
        })
        .map_or(true, |waypoint| waypoint.show_3d)
}

fn menu_layout(
    game: &Game<'_>,
    size: [f32; 2],
    menu: Menu,
    subtitle: Option<&str>,
    items: &[(&'static str, MenuAction)],
) -> ([f32; 4], Vec<[f32; 4]>) {
    let mut width = game.map_text_width(&menu_title(menu));
    if let Some(subtitle) = subtitle {
        width = width.max(game.map_text_width(subtitle));
    }
    for (label, _) in items {
        width = width.max(game.map_text_width(label));
    }
    let width = (width + 32.).min((size[0] - 12.).clamp(40., 340.));
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
        "路径点 {}",
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
            state.info_scroll = runtime::ListScroll::default();
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
                    "已复制坐标",
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
                color: default_waypoint_color(state),
                target: Some(target),
                rename: None,
                background: Mode::Map,
            });
            state.set_mode(Mode::Name);
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
                        "已复制目的地坐标",
                    )
                    .into(),
                    Instant::now(),
                ));
            }
        }
        MenuAction::EditWaypoint | MenuAction::TeleportWaypoint => {
            let controller = runtime::initialize(game.native_data_dir(runtime::MOD_ID));
            let points = controller.waypoints(&state.world);
            let Some(waypoint) = points
                .iter()
                .find(|w| Some(waypoint_spot(w)) == menu.waypoint)
            else {
                return;
            };
            if action == MenuAction::EditWaypoint {
                state.name_form = Some(waypoint_form(waypoint, Mode::Map));
                state.set_mode(Mode::Name);
                runtime::set_text_input(true);
            } else if teleport_allowed(game) {
                state.pending_command = Some(teleport_command(
                    waypoint.dimension,
                    [waypoint.x, waypoint.y, waypoint.z],
                    Dimension::from_id(player.dimension),
                ));
                state.close = true;
            }
        }
        MenuAction::MoveView => {}
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
                        "已删除路径点",
                    )
                    .into(),
                    Instant::now(),
                ));
            }
        }
        MenuAction::Toggle3D => {
            if let Some(spot) = menu.waypoint {
                let show_3d = waypoint_show_3d(state, spot);
                runtime::initialize(game.native_data_dir(runtime::MOD_ID)).set_waypoint_3d(
                    &state.world,
                    spot.dimension,
                    spot.x,
                    spot.z,
                    !show_3d,
                );
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
    let target = portal_target(state, menu, player.y.floor() as i32);
    let show_3d = menu
        .waypoint
        .map_or(true, |spot| waypoint_show_3d(state, spot));
    let items = menu_items(game, menu, target, show_3d);
    draw_context_menu(game, batch, menu, subtitle.as_deref(), &items);
}

fn draw_context_menu(
    game: &Game<'_>,
    batch: &mut HudBatch,
    menu: Menu,
    subtitle: Option<&str>,
    items: &[(&'static str, MenuAction)],
) {
    let has_subtitle = subtitle.is_some();
    let (rect, rows) = menu_layout(game, [batch.width, batch.height], menu, subtitle, items);
    let mouse = game.cursor();
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
    game.map_text(
        batch,
        &menu_title(menu),
        [rect[0] + 8.0, rect[1] + 5.0],
        0xffffff,
        true,
    );
    if let Some(subtitle) = subtitle {
        game.map_text(
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
    for ((label, action), row) in items.iter().zip(rows) {
        if inside(mouse, row) {
            game.native_fill(batch, row, 0xff2e3a48);
        }
        paint_button_icon(
            game,
            batch,
            action_icon(*action),
            [row[0] + 5., row[1] + 4.],
            true,
        );
        game.map_text(
            batch,
            &fit_text(game, label, row[2] - 23.),
            [row[0] + 18., row[1] + 4.],
            0xdddddd,
            false,
        );
    }
}

fn draw_notice(game: &Game<'_>, batch: &mut HudBatch, state: &State) {
    if let Some(error) = runtime::initialize(game.native_data_dir(runtime::MOD_ID))
        .maps
        .status()
    {
        let text = localized_format!(
            "Map cache: {}",
            "Кеш карты: {}",
            "Cache carte : {}",
            "Caché: {}",
            "Karten-Cache: {}",
            "地图缓存: {}",
            error
        );
        game.native_fill(
            batch,
            [0.0, batch.height - 20.0, batch.width, 20.0],
            0xe0402020,
        );
        game.map_text(batch, &text, [6.0, batch.height - 15.0], 0xffffd24a, false);
    }
    let Some((text, since)) = state.notice.as_ref() else {
        return;
    };
    if since.elapsed() > Duration::from_secs(2) {
        return;
    }
    let width = game.map_text_width(text);
    let x = (batch.width - width) / 2.0;
    let y = batch.height - 38.0;
    game.native_fill(batch, [x - 7.0, y - 4.0, width + 14.0, 14.0], 0xd01b2028);
    game.map_text(batch, text, [x, y], 0xffffd24a, true);
}

#[derive(Clone, Copy)]
enum ButtonIcon {
    Plain,
    Add,
    Back,
    Edit,
    Delete,
    View,
    Save,
    Cancel,
    Cache,
    Refresh,
    Previous,
    Next,
    Move,
    Copy,
    Teleport,
    Info,
    Toggle,
}

impl ButtonIcon {
    fn art(self) -> ([&'static str; 8], [u32; 3]) {
        match self {
            Self::Plain => (["........"; 8], [0; 3]),
            Self::Add => (
                [
                    "........", "...11...", "...12...", ".111221.", ".122221.", "...12...",
                    "...11...", "........",
                ],
                [0xff259756, 0xff94f4a8, 0xffe9ffef],
            ),
            Self::Back => (
                [
                    "........", "...1....", "..12....", ".122111.", "1222221.", ".122111.",
                    "..12....", "...1....",
                ],
                [0xff4383bc, 0xffb8e6ff, 0xffeefaff],
            ),
            Self::Edit => (
                [
                    ".....33.", "....3223", "...3213.", "..3213..", ".3213...", "3213....",
                    "213.....", "1.......",
                ],
                [0xff9d642d, 0xffffdf77, 0xfff6a74d],
            ),
            Self::Delete => (
                [
                    "...22...", ".222222.", "..1111..", "..1231..", "..1231..", "..1231..",
                    "..1111..", "........",
                ],
                [0xffbb3a4b, 0xffff8588, 0xfff75b67],
            ),
            Self::View => (
                [
                    "........", "..1111..", ".123321.", "12311321", "12311321", ".123321.",
                    "..1111..", "........",
                ],
                [0xff2a8a9d, 0xff89f0e0, 0xffd0fff8],
            ),
            Self::Save => (
                [
                    ".11111..", ".122211.", ".122311.", ".111111.", ".133331.", ".132231.",
                    ".133331.", ".111111.",
                ],
                [0xff427dc1, 0xff88b9f0, 0xffd2ecff],
            ),
            Self::Cancel => (
                [
                    "........", ".12..21.", "..1221..", "...22...", "...22...", "..1221..",
                    ".12..21.", "........",
                ],
                [0xffab415a, 0xffffa0a2, 0xffffd1cc],
            ),
            Self::Cache => (
                [
                    "..2222..", ".233332.", ".111111.", ".233332.", ".111111.", ".233332.",
                    "..1111..", "........",
                ],
                [0xff936730, 0xfff6b953, 0xffffe29b],
            ),
            Self::Refresh => (
                [
                    "..222...", ".21122..", "21...222", "2....132", "231....2", "222...12",
                    "..22112.", "...222..",
                ],
                [0xff328c7f, 0xff83dfb6, 0xffd2ffe4],
            ),
            Self::Previous => (
                [
                    "........", "....21..", "...21...", "..21....", "..21....", "...21...",
                    "....21..", "........",
                ],
                [0xff477eb1, 0xffa8d7ff, 0xffeefaff],
            ),
            Self::Next => (
                [
                    "........", "..12....", "...12...", "....12..", "....12..", "...12...",
                    "..12....", "........",
                ],
                [0xff477eb1, 0xffa8d7ff, 0xffeefaff],
            ),
            Self::Move => (
                [
                    "...22...", "...22...", ".11..11.", "22.33.22", "22.33.22", ".11..11.",
                    "...22...", "...22...",
                ],
                [0xff4678cb, 0xff8cd8ff, 0xfff0fbff],
            ),
            Self::Copy => (
                [
                    ".1111...", ".122111.", ".1221321", ".1111321", "...13321", "...13321",
                    "...11111", "........",
                ],
                [0xff7587c0, 0xffaebdf4, 0xffeceeff],
            ),
            Self::Teleport => (
                [
                    ".....22.", "....21..", "...21...", "..22222.", "...121..", "..121...",
                    ".121....", ".11.....",
                ],
                [0xffb773fe, 0xfff9d366, 0xfffff0c0],
            ),
            Self::Info => (
                [
                    "..1111..", ".112111.", "11111111", "11122111", "11112111", "11112111",
                    ".112211.", "..1111..",
                ],
                [0xff3e7db7, 0xffe4f8ff, 0xff84cffd],
            ),
            Self::Toggle => (
                [
                    "........", ".111111.", "12332221", "12332221", "12222221", ".111111.",
                    "........", "........",
                ],
                [0xff37766d, 0xff60a997, 0xffc2ffe0],
            ),
        }
    }
}

fn paint_button_icon(
    game: &Game<'_>,
    batch: &mut HudBatch,
    icon: ButtonIcon,
    at: [f32; 2],
    enabled: bool,
) {
    let (rows, colors) = icon.art();
    let colors = colors.map(|color| {
        if enabled {
            color
        } else {
            (color & 0x00ffffff) | 0x60000000
        }
    });
    for (y, row) in rows.iter().enumerate() {
        let pixels = row.as_bytes();
        let mut x = 0;
        while x < pixels.len() {
            let start = x;
            let value = pixels[x];
            x += 1;
            while x < pixels.len() && pixels[x] == value {
                x += 1;
            }
            if let Some(i) = value.checked_sub(b'1').filter(|i| *i < 3) {
                game.native_fill(
                    batch,
                    [
                        at[0] + start as f32,
                        at[1] + y as f32,
                        (x - start) as f32,
                        1.,
                    ],
                    colors[i as usize],
                );
            }
        }
    }
}

fn action_icon(action: MenuAction) -> ButtonIcon {
    match action {
        MenuAction::Copy | MenuAction::CopyDestination => ButtonIcon::Copy,
        MenuAction::Waypoint => ButtonIcon::Add,
        MenuAction::DeleteWaypoint => ButtonIcon::Delete,
        MenuAction::EditWaypoint => ButtonIcon::Edit,
        MenuAction::Teleport | MenuAction::TeleportWaypoint | MenuAction::TeleportDestination => {
            ButtonIcon::Teleport
        }
        MenuAction::MoveView => ButtonIcon::Move,
        MenuAction::Toggle3D => ButtonIcon::Toggle,
        MenuAction::Information => ButtonIcon::Info,
    }
}

fn button(
    game: &Game<'_>,
    batch: &mut HudBatch,
    icon: ButtonIcon,
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
    let icon_width = if matches!(icon, ButtonIcon::Plain) {
        0.
    } else if label.is_empty() {
        8.
    } else {
        12.
    };
    let label = fit_text(game, label, (rect[2] - icon_width - 6.).max(0.));
    let x = rect[0] + (rect[2] - game.map_text_width(&label) - icon_width) / 2.;
    if icon_width > 0. {
        paint_button_icon(
            game,
            batch,
            icon,
            [x.round(), (rect[1] + (rect[3] - 8.) / 2.).round()],
            enabled,
        );
    }
    game.map_text(
        batch,
        &label,
        [x + icon_width, rect[1] + (rect[3] - 10.) / 2.],
        text_color,
        false,
    );
}

fn waypoint_visible_rows(height: f32) -> usize {
    (((height - 90.0) / 42.0).floor() as usize).clamp(1, 12)
}

fn waypoint_row_at(state: &State, width: f32, height: f32, index: usize) -> Option<[f32; 4]> {
    let start = state.waypoint_scroll.offset(
        state
            .waypoints
            .len()
            .saturating_sub(waypoint_visible_rows(height)),
    );
    if index < start || index >= start + waypoint_visible_rows(height) {
        return None;
    }
    let row = index - start;
    Some([
        12.0,
        44.0 + row as f32 * 42.0,
        (width - 24.0).max(1.0),
        41.0,
    ])
}

/// The clickable 3D-visibility toggle at the right end of a waypoint row.
fn waypoint_3d_rect(row: [f32; 4]) -> [f32; 4] {
    [row[0] + row[2] - 24.0, row[1] + 10.0, 20.0, 18.0]
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
    state.set_mode(Mode::Map);
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
    game.map_text(
        batch,
        tr(
            "Waypoints",
            "Вейпоинты",
            "Repères",
            "Marcadores",
            "Wegpunkte",
            "路径点",
        ),
        [12.0, 8.0],
        0xffffff,
        true,
    );
    let mouse = game.cursor();
    let create = [batch.width - 260.0, 6.0, 120.0, 18.0];
    let back = [batch.width - 132.0, 6.0, 120.0, 18.0];
    button(
        game,
        batch,
        ButtonIcon::Add,
        create,
        tr(
            "Create here",
            "Создать здесь",
            "Créer ici",
            "Crear aquí",
            "Hier erstellen",
            "在此创建",
        ),
        mouse,
        true,
    );
    button(
        game,
        batch,
        ButtonIcon::Back,
        back,
        tr("Back", "Назад", "Retour", "Volver", "Zurück", "返回"),
        mouse,
        true,
    );
    game.native_fill(batch, [12.0, 28.0, batch.width - 24.0, 1.0], 0xff3a4553);
    if state.waypoints.is_empty() {
        game.map_text(
            batch,
            tr(
                "No waypoints yet. Right-click the map or choose “Create here”.",
                "Вейпоинтов пока нет. ПКМ на карте или «Создать здесь».",
                "Aucun repère. Faites un clic droit sur la carte ou choisissez « Créer ici ».",
                "Aún no hay marcadores. Haz clic derecho en el mapa o elige «Crear aquí».",
                "Noch keine Wegpunkte. Rechtsklick auf die Karte oder „Hier erstellen“ wählen.",
                "暂无路径点。右键点击地图，或选择“在此创建”。",
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
        let letter_x = row[0] + 6.0;
        if waypoint.icon == runtime::WaypointIcon::Skull {
            draw_skull(game, batch, [letter_x + 4.0, row[1] + 8.0], 10.0);
        }
        let letter = waypoint_text(state, index, waypoint);
        if waypoint.icon == runtime::WaypointIcon::Letter {
            game.map_text(
                batch,
                &letter,
                [letter_x, row[1] + 3.0],
                waypoint_argb(waypoint.color),
                false,
            );
        }
        let name_x = letter_x + game.map_text_width(&letter).max(6.0) + 6.0;
        game.map_text(
            batch,
            &waypoint.name,
            [name_x, row[1] + 3.0],
            0xffffff,
            false,
        );
        let biome = waypoint.biome.as_deref();
        let biome_x = name_x + game.map_text_width(&waypoint.name) + 7.0;
        let right = row[0] + row[2] - 30.0;
        if biome_x + runtime::BIOME_ICON_SIZE as f32 <= right {
            biome_icon(game, batch, biome, [biome_x, row[1] + 3.0]);
            if let Some(biome) = biome {
                let label_x = biome_x + 20.0;
                if label_x < right {
                    let label = fit_text(game, &localized_biome(biome), right - label_x);
                    game.map_text(batch, &label, [label_x, row[1] + 3.0], 0x8fcf86, false);
                }
            }
            if inside(mouse, [biome_x, row[1] + 2.0, right - biome_x, 17.0]) {
                let label = biome.map_or_else(
                    || tr("Biome: —", "биом: —", "Biome : —", "Bioma: —", "Biom: —", "生物群系：—").to_owned(),
                    localized_biome,
                );
                biome_tooltip = Some(label);
            }
        }
        dimension_icon(
            game,
            batch,
            waypoint.dimension,
            [row[0] + 6.0, row[1] + 22.0],
        );
        game.map_text(
            batch,
            &format!(
                "{}  {} {} {}",
                waypoint.dimension.label(),
                waypoint.x,
                waypoint.y,
                waypoint.z
            ),
            [row[0] + 26.0, row[1] + 25.0],
            0xaaaaaa,
            false,
        );
        let toggle = waypoint_3d_rect(row);
        let on = waypoint.show_3d;
        game.native_fill(batch, toggle, if on { 0xff2e7d52 } else { 0xff2a3542 });
        if inside(mouse, toggle) {
            game.native_fill(batch, toggle, 0xff3a4a5c);
        }
        let label = tr("3D", "3D", "3D", "3D", "3D", "3D");
        paint_button_icon(
            game,
            batch,
            ButtonIcon::Toggle,
            [toggle[0] + 3., toggle[1] + 3.],
            on,
        );
        game.map_text(
            batch,
            label,
            [
                toggle[0] + 6. + (toggle[2] - game.map_text_width(label)) / 2.0,
                toggle[1] + 3.0,
            ],
            if on { 0xffffff } else { 0x707880 },
            false,
        );
    }
    let bottom = batch.height - 42.0;
    let selected = state
        .waypoint_selected
        .and_then(|index| state.waypoints.get(index));
    let [rename, delete, show] = waypoint_actions(batch.width, batch.height);
    button(
        game,
        batch,
        ButtonIcon::Edit,
        rename,
        tr("Edit", "Изменить", "Modifier", "Editar", "Bearbeiten", "编辑"),
        mouse,
        selected.is_some(),
    );
    button(
        game,
        batch,
        ButtonIcon::Delete,
        delete,
        tr("Delete", "Удалить", "Supprimer", "Eliminar", "Löschen", "删除"),
        mouse,
        selected.is_some(),
    );
    button(
        game,
        batch,
        ButtonIcon::Move,
        show,
        tr(
            "Show on map",
            "Показать на карте",
            "Voir sur la carte",
            "Ver en el mapa",
            "Auf Karte zeigen",
            "在地图上显示",
        ),
        mouse,
        selected.is_some(),
    );
    let hint = tr(
        "Select a waypoint to see its colour, name, biome and coordinates",
        "Выберите точку, чтобы увидеть её цвет, имя, биом и координаты",
        "Sélectionnez un repère pour voir sa couleur, son nom, son biome et ses coordonnées",
        "Selecciona un marcador para ver su color, nombre, bioma y coordenadas",
        "Wegpunkt auswählen, um Farbe, Namen, Biom und Koordinaten anzuzeigen",
        "选择路径点可查看颜色、名称、生物群系和坐标",
    );
    game.map_text(batch, hint, [12.0, bottom + 24.0], 0x888f98, true);
    draw_notice(game, batch, state);
    if let Some(label) = biome_tooltip {
        draw_tooltip(game, batch, &label, mouse);
    }
}

const NAME_FORM_HEIGHT: f32 = 126.0;

fn name_form_rect(viewport_width: f32, height: f32) -> [f32; 4] {
    let width = 340.0_f32.min(viewport_width - 24.0);
    [
        (viewport_width - width) / 2.0,
        height / 2.0 - NAME_FORM_HEIGHT / 2.0,
        width,
        NAME_FORM_HEIGHT,
    ]
}

fn color_swatch_at(rect: [f32; 4], index: usize) -> [f32; 4] {
    [
        rect[0] + 10.0 + index as f32 * 22.0,
        rect[1] + 61.0,
        18.0,
        18.0,
    ]
}

fn draw_name_form(game: &Game<'_>, batch: &mut HudBatch, state: &State, form: &NameForm) {
    match form.background {
        Mode::Map => draw_map_screen(game, batch, state),
        Mode::Waypoints => draw_waypoints_screen(game, batch, state),
        Mode::Name => {}
        Mode::Caches => draw_cache_screen(game, batch, state),
    }
    draw_name_panel(game, batch, form);
}

fn name_form_buttons(rect: [f32; 4]) -> ([f32; 4], [f32; 4]) {
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
    (save, cancel)
}

fn draw_name_panel(game: &Game<'_>, batch: &mut HudBatch, form: &NameForm) {
    game.native_fill(batch, [0.0, 0.0, batch.width, batch.height], 0x70000000);
    let rect = name_form_rect(batch.width, batch.height);
    game.native_fill(batch, rect, 0xf01b2028);
    game.native_fill(batch, [rect[0], rect[1], rect[2], 1.0], 0xff3a4553);
    game.native_fill(
        batch,
        [rect[0], rect[1] + rect[3] - 1.0, rect[2], 1.0],
        0xff3a4553,
    );
    game.map_text(
        batch,
        if form.rename.is_some() {
            tr(
                "Edit waypoint",
                "Изменить вейпоинт",
                "Modifier le repère",
                "Editar marcador",
                "Wegpunkt bearbeiten",
                "编辑路径点",
            )
        } else {
            tr(
                "Waypoint name",
                "Название вейпоинта",
                "Nom du repère",
                "Nombre del marcador",
                "Wegpunktname",
                "路径点名称",
            )
        },
        [rect[0] + 10.0, rect[1] + 8.0],
        0xffffff,
        true,
    );
    let field = [rect[0] + 10.0, rect[1] + 26.0, rect[2] - 20.0, 18.0];
    game.native_fill(batch, field, 0xff0d1117);
    game.map_text(
        batch,
        &format!("{}_", form.text),
        [field[0] + 4.0, field[1] + 5.0],
        0xffffff,
        false,
    );
    game.map_text(
        batch,
        tr("Colour", "Цвет", "Couleur", "Color", "Farbe", "颜色"),
        [rect[0] + 10.0, rect[1] + 49.0],
        0xaaaaaa,
        false,
    );
    for (index, &color) in runtime::WAYPOINT_COLORS.iter().enumerate() {
        let swatch = color_swatch_at(rect, index);
        if form.color == color {
            game.native_fill(
                batch,
                [
                    swatch[0] - 2.0,
                    swatch[1] - 2.0,
                    swatch[2] + 4.0,
                    swatch[3] + 4.0,
                ],
                0xffffffff,
            );
        }
        game.native_fill(batch, swatch, waypoint_argb(color));
    }
    let mouse = game.cursor();
    let (save, cancel) = name_form_buttons(rect);
    button(
        game,
        batch,
        ButtonIcon::Save,
        save,
        tr("Save", "Сохранить", "Enregistrer", "Guardar", "Speichern", "保存"),
        mouse,
        true,
    );
    button(
        game,
        batch,
        ButtonIcon::Cancel,
        cancel,
        tr("Cancel", "Отмена", "Annuler", "Cancelar", "Abbrechen", "取消"),
        mouse,
        true,
    );
    game.map_text(
        batch,
        tr(
            "Enter — save, Esc — cancel",
            "Enter — сохранить, Esc — отмена",
            "Entrée — enregistrer, Échap — annuler",
            "Intro — guardar, Esc — cancelar",
            "Eingabe — speichern, Esc — abbrechen",
            "回车保存，Esc 取消",
        ),
        [rect[0] + 10.0, rect[1] + 86.0],
        0x888f98,
        false,
    );
}

fn draw_tooltip(game: &Game<'_>, batch: &mut HudBatch, text: &str, at: [f32; 2]) {
    let width = game.map_text_width(text);
    let x = (at[0] + 10.0).min(batch.width - width - 10.0).max(4.0);
    let y = (at[1] - 18.0).max(4.0);
    game.native_fill(batch, [x - 4.0, y - 3.0, width + 8.0, 13.0], 0xf01b2028);
    game.map_text(batch, text, [x, y], 0xffffd24a, true);
}

// Hit-test the full short label of the waypoint under the map pointer.
fn waypoint_at_pointer<'a>(
    game: &Game<'_>,
    state: &'a State,
    view: &View,
    rect: [f32; 4],
    mouse: [f32; 2],
) -> Option<&'a Waypoint> {
    waypoint_hit(
        game,
        &state.waypoints,
        &state.waypoint_labels,
        view,
        rect,
        mouse,
    )
}

fn waypoint_hit<'a>(
    game: &Game<'_>,
    points: &'a [Waypoint],
    labels: &[String],
    view: &View,
    rect: [f32; 4],
    mouse: [f32; 2],
) -> Option<&'a Waypoint> {
    points
        .iter()
        .enumerate()
        .filter_map(|(index, waypoint)| {
            if waypoint.dimension != view.dimension {
                return None;
            }
            let pixel = view.screen_at([f64::from(waypoint.x) + 0.5, f64::from(waypoint.z) + 0.5]);
            let dx = mouse[0] - (rect[0] + pixel[0] as f32 * rect[2] / view.width as f32);
            let dy = mouse[1] - (rect[1] + pixel[1] as f32 * rect[3] / view.height as f32);
            let radius = if waypoint.icon == runtime::WaypointIcon::Letter {
                labels
                    .get(index)
                    .map_or(7., |label| (game.map_text_width(label) + 4.).max(14.) / 2.)
            } else {
                7.
            };
            (dx.abs() <= radius && dy.abs() <= 7.).then_some((waypoint, dx * dx + dy * dy))
        })
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(waypoint, _)| waypoint)
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

fn world_screen(view: &View, rect: [f32; 4], at: [f64; 2]) -> [f32; 2] {
    let pixel = view.screen_at(at);
    [
        rect[0] + pixel[0] as f32 * rect[2] / view.width as f32,
        rect[1] + pixel[1] as f32 * rect[3] / view.height as f32,
    ]
}

type ScreenSegment = ([f32; 2], [f32; 2]);

fn draw_segments(
    game: &Game<'_>,
    batch: &mut HudBatch,
    rect: [f32; 4],
    segments: &[ScreenSegment],
    color: u32,
) {
    for &(from, to) in segments {
        draw_line(game, batch, from, to, rect, color);
    }
}

fn world_box_segments(view: &View, rect: [f32; 4], bounds: [f64; 4]) -> Vec<ScreenSegment> {
    let [x0, z0, x1, z1] = bounds;
    let corners = [[x0, z0], [x1, z0], [x1, z1], [x0, z1]];
    let points = corners.map(|corner| world_screen(view, rect, corner));
    (0..4)
        .map(|index| (points[index], points[(index + 1) % 4]))
        .collect()
}

fn draw_world_box(
    game: &Game<'_>,
    batch: &mut HudBatch,
    view: &View,
    rect: [f32; 4],
    bounds: [f64; 4],
    color: u32,
) {
    draw_segments(
        game,
        batch,
        rect,
        &world_box_segments(view, rect, bounds),
        color,
    );
}

fn circle_segments(
    view: &View,
    rect: [f32; 4],
    center: [f64; 2],
    radius: f64,
) -> Vec<ScreenSegment> {
    if radius <= 0.0 {
        return Vec::new();
    }
    let count = 96;
    let mut segments = Vec::with_capacity(count);
    let mut previous = world_screen(view, rect, [center[0] + radius, center[1]]);
    for index in 1..=count {
        let angle = std::f64::consts::TAU * index as f64 / count as f64;
        let point = world_screen(
            view,
            rect,
            [
                center[0] + angle.cos() * radius,
                center[1] + angle.sin() * radius,
            ],
        );
        segments.push((previous, point));
        previous = point;
    }
    segments
}

// The outer edges of the chunks the game actually draws at this radius,
// including the slack `in_view_distance` allows on each axis.
fn chunk_border_segments(
    view: &View,
    rect: [f32; 4],
    center: (i32, i32),
    radius: i32,
) -> Vec<ScreenSegment> {
    if radius < 1 {
        return Vec::new();
    }
    let mut rows = Vec::new();
    for dz in -radius..=radius {
        let mut min_dx = None;
        let mut max_dx = None;
        for dx in -radius..=radius {
            if horizon_client::streaming::in_view_distance((0, 0), radius, dx, dz) {
                min_dx = Some(min_dx.map_or(dx, |value: i32| value.min(dx)));
                max_dx = Some(max_dx.map_or(dx, |value: i32| value.max(dx)));
            }
        }
        if let (Some(min_dx), Some(max_dx)) = (min_dx, max_dx) {
            rows.push((dz, min_dx, max_dx));
        }
    }
    let Some(first) = rows.first().copied() else {
        return Vec::new();
    };
    let last = *rows.last().unwrap();
    let world = |dx: i32, dz: i32| {
        [
            f64::from(center.0 + dx) * 16.0,
            f64::from(center.1 + dz) * 16.0,
        ]
    };
    let mut segments = Vec::new();
    segments.push((
        world_screen(view, rect, world(first.1, first.0)),
        world_screen(view, rect, world(first.2 + 1, first.0)),
    ));
    segments.push((
        world_screen(view, rect, world(last.2 + 1, last.0 + 1)),
        world_screen(view, rect, world(last.1, last.0 + 1)),
    ));
    for (index, &(dz, min_dx, max_dx)) in rows.iter().enumerate() {
        let left = world(min_dx, dz);
        let right = world(max_dx + 1, dz);
        segments.push((
            world_screen(view, rect, left),
            world_screen(view, rect, [left[0], left[1] + 16.0]),
        ));
        segments.push((
            world_screen(view, rect, right),
            world_screen(view, rect, [right[0], right[1] + 16.0]),
        ));
        if let Some(&(_, next_min, next_max)) = rows.get(index + 1) {
            if next_min != min_dx {
                segments.push((
                    world_screen(view, rect, [left[0], left[1] + 16.0]),
                    world_screen(view, rect, [world(next_min, dz + 1)[0], left[1] + 16.0]),
                ));
            }
            if next_max != max_dx {
                segments.push((
                    world_screen(view, rect, [right[0], right[1] + 16.0]),
                    world_screen(
                        view,
                        rect,
                        [world(next_max + 1, dz + 1)[0], right[1] + 16.0],
                    ),
                ));
            }
        }
    }
    segments
}

fn segment_distance(point: [f32; 2], from: [f32; 2], to: [f32; 2]) -> f32 {
    let vx = to[0] - from[0];
    let vy = to[1] - from[1];
    let wx = point[0] - from[0];
    let wy = point[1] - from[1];
    let length = vx * vx + vy * vy;
    let t = if length > 0.0 {
        ((wx * vx + wy * vy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let dx = point[0] - (from[0] + vx * t);
    let dy = point[1] - (from[1] + vy * t);
    (dx * dx + dy * dy).sqrt()
}

fn segments_distance(point: [f32; 2], segments: &[ScreenSegment]) -> f32 {
    segments
        .iter()
        .map(|&(from, to)| segment_distance(point, from, to))
        .fold(f32::INFINITY, f32::min)
}

fn draw_border_hint(
    game: &Game<'_>,
    batch: &mut HudBatch,
    text: &str,
    mouse: [f32; 2],
    rect: [f32; 4],
) {
    if !inside(mouse, rect) {
        return;
    }
    let width = game.map_text_width(text);
    let x = (mouse[0] + 12.0)
        .min(rect[0] + rect[2] - width - 6.0)
        .max(rect[0] + 4.0);
    let y = (mouse[1] - 16.0).max(rect[1] + 4.0);
    game.native_fill(batch, [x - 4.0, y - 3.0, width + 8.0, 13.0], 0xe010151d);
    game.map_text(batch, text, [x, y], 0xffff9a9a, true);
}

fn draw_border(
    game: &Game<'_>,
    batch: &mut HudBatch,
    view: &View,
    rect: [f32; 4],
    mouse: Option<[f32; 2]>,
) {
    let border_color = 0xffe04040;
    let mut labels = Vec::<(String, Vec<ScreenSegment>)>::new();
    if matches!(view.dimension, Dimension::Overworld | Dimension::Nether) {
        let [mut min_x, mut min_z, mut max_x, mut max_z] = game.border.bounds();
        if view.dimension == Dimension::Nether {
            min_x /= 8.0;
            min_z /= 8.0;
            max_x /= 8.0;
            max_z /= 8.0;
        }
        let segments = world_box_segments(view, rect, [min_x, min_z, max_x, max_z]);
        draw_segments(game, batch, rect, &segments, border_color);
        if view.large {
            labels.push((
                tr(
                    "World border",
                    "Граница мира",
                    "Bordure du monde",
                    "Borde del mundo",
                    "Weltgrenze",
                    "世界边界",
                )
                .to_owned(),
                segments,
            ));
        }
    }

    let Some(player) = game.native_player() else {
        return;
    };
    if view.dimension != Dimension::from_id(player.dimension) {
        return;
    }
    let center = game.streaming.center;
    let center_blocks = [
        f64::from(center.0) * 16.0 + 8.0,
        f64::from(center.1) * 16.0 + 8.0,
    ];
    // The game's view still shows the first ring outside the nominal radius
    // while chunks arrive, so draw one extra chunk to contain it.
    let chunk_radius = match &game.remote {
        Some(remote) => game.streaming.radius.min(remote.view_distance + 1),
        None => game.streaming.radius,
    }
    .max(0)
        + 1;
    let full_radius = if game.lod.is_some() {
        chunk_radius.max(game.lod_distance() as i32)
    } else {
        chunk_radius
    };
    let chunk_segments = chunk_border_segments(view, rect, center, chunk_radius);
    draw_segments(game, batch, rect, &chunk_segments, border_color);
    if view.large {
        labels.push((
            localized_format!(
                "Chunk render: {} chunks",
                "Прорисовка чанков: {} чанков",
                "Rendu des chunks : {} chunks",
                "Render de chunks: {} chunks",
                "Chunk-Sichtweite: {} Chunks",
                "区块渲染距离：{} 区块",
                chunk_radius
            ),
            chunk_segments,
        ));
    }
    if full_radius > chunk_radius {
        let lod_segments =
            circle_segments(view, rect, center_blocks, f64::from(full_radius) * 16.0);
        draw_segments(game, batch, rect, &lod_segments, border_color);
        if view.large {
            labels.push((
                localized_format!(
                    "Actual render (LOD): {} chunks",
                    "Фактическая прорисовка (LOD): {} чанков",
                    "Rendu réel (LOD) : {} chunks",
                    "Render real (LOD): {} chunks",
                    "Tatsächliche Sichtweite (LOD): {} Chunks",
                    "实际渲染距离（LOD）：{} 区块",
                    full_radius
                ),
                lod_segments,
            ));
        }
    }
    if view.large
        && let Some(mouse) = mouse
    {
        let mut nearest: Option<(f32, &String)> = None;
        for (text, segments) in &labels {
            let distance = segments_distance(mouse, segments);
            let closer = nearest.is_none_or(|(best, _)| distance < best);
            if distance <= 8.0 && closer {
                nearest = Some((distance, text));
            }
        }
        if let Some((_, text)) = nearest {
            draw_border_hint(game, batch, text, mouse, rect);
        }
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

fn project(camera: &mod_api::Camera, world: [f64; 3], size: [f32; 2]) -> Option<[f32; 2]> {
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

/// The exact camera the game uses this frame, rebuilt from its own view state.
/// `native_camera` only returns a frame when a trait mod (`mod_api::mods`) is
/// registered; this injection-based mod has none, so it reads the same camera
/// position, rotation and field of view (including FOV/zoom changes) directly.
fn world_camera(
    game: &Game<'_>,
    player: &crate::game::native_hooks::NativePlayer,
    size: [f32; 2],
) -> mod_api::Camera {
    let partial = game.clock.camera_fraction() as f32;
    let position = game.view.position;
    let yaw = game.view.y_rot;
    let pitch = game.view.x_rot;
    let fov = game.fov_degrees(partial, position);
    mod_api::Camera {
        position,
        eye_height: game.view.eye_height,
        partial_tick: partial,
        yaw,
        pitch,
        fov_degrees: fov,
        near: 0.05,
        far: 2048.0,
        // Match `Game::render` exactly: the bob and spin matrices go into the
        // view, so markers stay glued to the world instead of drifting as the
        // camera sways.
        view: (game.bob_matrix(partial)
            * game.spinning_matrix(partial)
            * horizon_client::camera::view_matrix(yaw, pitch))
        .to_cols_array(),
        projection: horizon_client::camera::perspective(fov, size[0], size[1], 2048.0, 0.05, true),
        dimension: player.dimension.to_owned(),
    }
}

fn waypoint_label_left(x: f32, marker_width: f32, text_width: f32, viewport_width: f32) -> f32 {
    let gap = marker_width / 2. + 4.;
    if x + text_width + gap + 3. > viewport_width {
        x - text_width - gap
    } else {
        x + gap
    }
}

fn draw_world_waypoints(
    game: &Game<'_>,
    batch: &mut HudBatch,
    state: &State,
    settings: &runtime::minimap::Settings,
) {
    if !settings.world_waypoints {
        return;
    }
    let Some(player) = game.native_player() else {
        return;
    };
    let camera = world_camera(game, &player, [batch.width, batch.height]);
    let dimension = Dimension::from_id(player.dimension);
    let style = settings.world_waypoint_style;
    // In the 3D view the pointer is the crosshair at the screen centre: a label
    // is "hovered" when its marker sits under it. The radius is forgiving so a
    // marker just off the crosshair still counts.
    let crosshair = [batch.width / 2.0, batch.height / 2.0];
    const HOVER_RADIUS: f32 = 24.0;
    for (index, waypoint) in state.waypoints.iter().enumerate() {
        if !waypoint.show_3d || waypoint.dimension != dimension {
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
        let letter = waypoint_text(state, index, waypoint);
        let width = if waypoint.icon == runtime::WaypointIcon::Letter {
            (game.map_text_width(&letter) + 4.).max(10.)
        } else {
            10.
        };
        game.native_fill(
            batch,
            [x - width / 2. - 1., y - 6.0, width + 2., 12.0],
            0xff101010,
        );
        game.native_fill(
            batch,
            [x - width / 2., y - 5.0, width, 10.0],
            waypoint_argb(waypoint.color),
        );
        if waypoint.icon == runtime::WaypointIcon::Skull {
            draw_skull(game, batch, [x, y], 9.5);
        } else {
            game.map_text(
                batch,
                &letter,
                [x - game.map_text_width(&letter) / 2.0, y - 4.0],
                0xff201800,
                false,
            );
        }
        // The coloured marker is always drawn; the label below it depends on the
        // selected style. Distance changes each frame, so it is only composed
        // when the style asks for it and, with "labels only on hover", when the
        // crosshair is over the marker.
        let hovered =
            (x - crosshair[0]).abs() <= HOVER_RADIUS && (y - crosshair[1]).abs() <= HOVER_RADIUS;
        let label = if !settings.labels_on_hover || hovered {
            match style {
                runtime::minimap::WorldMarkerStyle::Letter => None,
                runtime::minimap::WorldMarkerStyle::LetterDistance
                | runtime::minimap::WorldMarkerStyle::NameDistance => {
                    let dx = f64::from(waypoint.x) + 0.5 - player.x;
                    let dy = f64::from(waypoint.y) + 1.5 - player.y;
                    let dz = f64::from(waypoint.z) + 0.5 - player.z;
                    let distance = (dx * dx + dy * dy + dz * dz).sqrt().round() as i64;
                    match style {
                        runtime::minimap::WorldMarkerStyle::NameDistance => Some(format!(
                            "{}  {} {}",
                            waypoint.name,
                            distance,
                            tr("m", "м", "m", "m", "m", "米")
                        )),
                        _ => Some(format!("{distance} {}", tr("m", "м", "m", "m", "m", "米"))),
                    }
                }
            }
        } else {
            None
        };
        if let Some(label) = label {
            let marker_width = width;
            let width = game.map_text_width(&label);
            let label_x = waypoint_label_left(x, marker_width, width, batch.width);
            let label_y = y - 4.0;
            game.native_fill(
                batch,
                [label_x - 3.0, label_y - 2.0, width + 6.0, 11.0],
                0xd010151d,
            );
            let label_color = if waypoint.icon == runtime::WaypointIcon::Skull {
                0xffe9e6dc
            } else {
                waypoint_argb(waypoint.color)
            };
            game.map_text(batch, &label, [label_x, label_y], label_color, true);
        }
    }
}

fn large_camera_view(
    session: u64,
    dimension: Dimension,
    layer: Layer,
    camera: &maps::Camera,
    rect: [f32; 4],
    scale: f32,
) -> View {
    View {
        session,
        dimension,
        layer,
        center: camera.center,
        span: [
            f64::from(rect[2]) * camera.zoom,
            f64::from(rect[3]) * camera.zoom,
        ],
        width: ((rect[2] * scale.max(1.)).ceil() as usize).max(1),
        height: ((rect[3] * scale.max(1.)).ceil() as usize).max(1),
        angle: 0.,
        round: false,
        large: true,
    }
}

fn camera_target(view: &View, camera: &maps::Camera) -> View {
    let mut target = view.clone();
    target.center = camera.target;
    if view.large {
        target.span = view
            .span
            .map(|size| size * (camera.zoom_target / camera.zoom));
    }
    target
}

// Camera motion never waits for terrain preparation. Newly exposed areas may
// be empty until the next atlas arrives; existing tiles retain world positions.
fn complete_map_view(desired: View, prepared: &View) -> View {
    if desired.session != prepared.session
        || desired.dimension != prepared.dimension
        || desired.layer != prepared.layer
        || desired.large != prepared.large
    {
        return prepared.clone();
    }
    desired
}

fn waypoint_identity(identity: &super::minimap_compat::ClientState) -> Option<String> {
    use super::minimap_compat::WorldId as NativeWorldId;
    match identity.world.as_ref()? {
        NativeWorldId::Singleplayer(folder) => runtime::waypoint_world(true, folder),
        NativeWorldId::Server(address) => runtime::waypoint_world(false, address),
    }
}

/// Hears the server's death screen packet. The player has not respawned yet,
/// so their current block position is the place they died.
pub(super) fn client_packet(
    game: &mut Game<'_>,
    packet: &horizon_protocol::packets::play::Clientbound,
) {
    use horizon_protocol::packets::play::Clientbound;
    if !game.native_window() || !game.map_on(runtime::MOD_ID) {
        return;
    }
    match packet {
        // The chunk is handled by the game after this hook, so sampling waits
        // for the next frame instead of reading a chunk that is still absent.
        Clientbound::LevelChunkWithLight(chunk) => {
            state().biome_chunks.insert((chunk.x, chunk.z));
        }
        Clientbound::PlayerCombatKill(_) => {
            let controller = runtime::initialize(game.native_data_dir(runtime::MOD_ID));
            if !controller.settings().death_waypoints {
                return;
            }
            let Some(player) = game.native_player() else {
                return;
            };
            let Some(identity) = game.map_client_state() else {
                return;
            };
            let Some(world) = waypoint_identity(&identity) else {
                return;
            };
            let dimension = Dimension::from_id(player.dimension);
            if dimension == Dimension::Player {
                return;
            }
            let at = [
                player.x.floor() as i32,
                player.y.floor() as i32,
                player.z.floor() as i32,
            ];
            let biome = known_biome(game, dimension, at);
            controller.add_death_waypoint(&world, dimension, at[0], at[1], at[2], biome.as_deref());
        }
        _ => {}
    }
}

fn apply_sdk_lifecycle(game: &Game<'_>) {
    if !game.native_window() {
        return;
    }
    let pending = runtime::CLIENT_EVENTS.take();
    if pending.disconnected {
        disconnect(game);
    }
    if pending.resources_reloaded {
        atlas_invalidated();
    }
}

pub(super) fn frame(game: &mut Game<'_>, elapsed: Duration) {
    let _timing = runtime::profiling::measure("client-frame");
    apply_sdk_lifecycle(game);
    if !game.native_window() {
        return;
    }
    let controller = runtime::initialize(game.native_data_dir(runtime::MOD_ID));
    if !game.map_on(runtime::MOD_ID) {
        if game.mod_screen() == Some(runtime::MOD_ID) {
            game.mod_screen_close(runtime::MOD_ID);
        }
        return;
    }
    let Some(player) = game.native_player() else {
        return;
    };
    let Some(identity) = game.map_client_state() else {
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
    let sprites_version = game.block_sprites_version();
    if state.leaves.as_ref().map(|(version, _)| *version) != Some(sprites_version) {
        state.leaves = Some((sprites_version, Arc::new(build_leaf_palette(game))));
    }
    if state.close {
        state.close = false;
        runtime::set_text_input(false);
        game.mod_screen_close(runtime::MOD_ID);
    }
    if game.mod_screen() != Some(runtime::MOD_ID) {
        state.caches.preview = None;
        runtime::set_text_input(false);
        if state.mode == Mode::Name {
            state.set_mode(Mode::Map);
            state.name_form = None;
        }
    }
    if let Some(request) = runtime::take_screen_request() {
        let desired = match request {
            runtime::ScreenRequest::Map => Mode::Map,
            runtime::ScreenRequest::Waypoints => Mode::Waypoints,
        };
        // The key helper opens the screen; this side only chooses its mode.
        if game.mod_screen() == Some(runtime::MOD_ID) {
            state.set_mode(desired);
            state.info = None;
            state.name_form = None;
            state.menu = None;
            state.waypoint_selected = None;
            runtime::set_text_input(false);
        }
    }
    state.portals = controller.maps.portals();
    let world = waypoint_identity(&identity).unwrap_or_default();
    if state.world != world {
        let next = state.session + 1;
        *state = State {
            world,
            session: next,
            ..State::default()
        };
        runtime::set_waypoint_background_transparent(false);
        controller.maps.reset(next);
    }
    update_cache_ui(game, &mut state);
    state.waypoints = controller.waypoints(&state.world);
    let dimension = Dimension::from_id(player.dimension);
    controller.maps.configure(
        state.session,
        &state.world,
        dimension,
        controller.settings().cache_ram_mib,
    );
    if !state.world.is_empty() {
        controller.maps.remember_position(
            state.session,
            runtime::maps::LastPosition {
                dimension,
                block: [
                    player.x.floor() as i32,
                    player.y.floor() as i32,
                    player.z.floor() as i32,
                ],
                yaw: player
                    .yaw
                    .is_finite()
                    .then(|| player.yaw.rem_euclid(360.).round() as i16),
            },
        );
    }
    if state.dimension != dimension {
        state.lod_sources.clear();
        state.frame = None;
        state.portals.clear();

        state.seen.clear();
        state.chunk_keys.clear();
        state.chunk_revision = 0;
        state.chunk_cursor = None;
        state.dimension = dimension;
    }
    let head = (player.y + 1.62).floor() as i32;
    let covered = game
        .map_light([player.x.floor() as i32, head, player.z.floor() as i32])
        .1
        == 0;
    let layer = if dimension == Dimension::Nether || covered {
        Layer::Below(head)
    } else {
        Layer::Surface
    };
    state.layers.insert(dimension, layer);
    let settings = controller.settings();
    if settings.death_waypoint_auto_remove && dimension != Dimension::Player {
        let reached: Vec<_> = state
            .waypoints
            .iter()
            .filter(|waypoint| {
                waypoint.dimension == dimension
                    && runtime::death_waypoint_reached([player.x, player.y, player.z], waypoint)
            })
            .map(|waypoint| (waypoint.x, waypoint.z))
            .collect();
        if !reached.is_empty() {
            for (x, z) in reached {
                controller.remove_waypoint(&state.world, dimension, x, z);
            }
            state.waypoints = controller.waypoints(&state.world);
        }
    }
    let screen_open = game.mod_screen() == Some(runtime::MOD_ID);
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
    if state.lod_elapsed >= 0.05 {
        state.lod_elapsed = 0.0;
        if let Some(lod) = game.lod.as_ref() {
            let _timing = runtime::profiling::measure("lod-snapshot-pick");
            state
                .lod_sources
                .retain(|_, (source, _)| source.strong_count() != 0);
            let known = known_lod_sources(&state, dimension);
            let span = controller
                .maps
                .requested()
                .filter(|v| v.dimension == dimension)
                .map_or([256.; 2], |v| v.span);
            let focus = if selected == dimension {
                state.camera.target
            } else {
                [player.x, player.z]
            };
            for (key, columns) in super::minimap_lod_adapter::snapshots(lod, &known, focus, span, 8)
            {
                let source = Arc::downgrade(&columns);
                let biome_palette = game
                    .world_biomes
                    .as_ref()
                    .unwrap_or_else(|| game.res.biomes())
                    .clone();
                let leaves = state.leaves.as_ref().map(|(_, leaves)| Arc::clone(leaves));
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
                            let [origin_x, origin_z, size, _] = key.bounds();
                            let stride = size / 16;
                            let biome_at = |wx: i32, wz: i32| -> Option<u16> {
                                let dx = i64::from(wx) - origin_x;
                                let dz = i64::from(wz) - origin_z;
                                if dx < 0 || dz < 0 || dx >= size || dz >= size {
                                    return None;
                                }
                                let cell = ((dz / stride) * 16 + (dx / stride)) as usize;
                                columns.get(cell)?.surface.map(|surface| surface.biome)
                            };
                            let color = |id: horizon_world::registry::BlockStateId,
                                         at: [i32; 2]| {
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
                                let leaf = leaves
                                    .as_ref()
                                    .and_then(|leaves| leaves.states.get(&id.0))
                                    .copied();
                                let water =
                                    matches!(state.fluid, horizon_client::blocks::Fluid::Water);
                                let needs_grass = leaf
                                    .is_some_and(|leaf| leaf.tint == LeafTint::Grass)
                                    || maps::needs_grass_tint(state.name.as_ref());
                                let needs_foliage = leaf
                                    .is_some_and(|leaf| leaf.tint == LeafTint::Foliage)
                                    || maps::needs_foliage_tint(state.name.as_ref());
                                let blend = blend_biome_tints(
                                    &biome_palette,
                                    at,
                                    BIOME_BLEND_RADIUS,
                                    BIOME_BLEND_STEP,
                                    needs_grass,
                                    needs_foliage,
                                    water,
                                    |wx, wz| biome_at(wx, wz),
                                );
                                let rgb = if water {
                                    blend
                                        .water
                                        .unwrap_or_else(|| if base == 0 { 0x3f76e4 } else { base })
                                } else {
                                    leaf.and_then(|leaf| leaf.map_color(blend.grass, blend.foliage))
                                        .unwrap_or_else(|| {
                                            plant_color(state.name.as_ref()).unwrap_or_else(|| {
                                                maps::block_map_color(
                                                    state.name.as_ref(),
                                                    base,
                                                    blend.grass,
                                                    blend.foliage,
                                                )
                                            })
                                        })
                                };
                                Some(rgb)
                            };
                            let mut tile = maps::Tile {
                                pixels: vec![0; 1024],
                                biomes: vec![None; 256],
                                ..maps::Tile::default()
                            };
                            for (i, column) in columns.iter().enumerate() {
                                let Some(surface) = column.surface else {
                                    continue;
                                };
                                let at = [
                                    (origin_x + (i as i64 % 16) * stride + stride / 2) as i32,
                                    (origin_z + (i as i64 / 16) * stride + stride / 2) as i32,
                                ];
                                let height = surface
                                    .water
                                    .map_or(surface.height, |(h, _)| h)
                                    .saturating_sub(1);
                                let snows = dimension == Dimension::Overworld
                                    && biome_palette.list.get(surface.biome as usize).is_some_and(
                                        |b| b.snows_at(at[0], height, at[1]) == Some(true),
                                    );
                                let snow = leaves.as_ref().map_or(0xffffff, |p| p.snow);
                                let ice = leaves.as_ref().map_or(0xa0a0ff, |p| p.ice);
                                let base = color(surface.state, at);
                                let rgb = if let Some((height, fluid)) = surface.water {
                                    let Some(fluid_color) = color(fluid, at) else {
                                        continue;
                                    };
                                    if book.states.get(fluid.0 as usize).is_some_and(|s| {
                                        matches!(s.fluid, horizon_client::blocks::Fluid::Water)
                                    }) {
                                        maps::lod_water_color(
                                            base,
                                            fluid_color,
                                            height.saturating_sub(surface.height).max(0) as u32,
                                            snows,
                                            ice,
                                        )
                                    } else {
                                        fluid_color
                                    }
                                } else {
                                    let Some(base) = base else {
                                        continue;
                                    };
                                    let name = book
                                        .states
                                        .get(surface.state.0 as usize)
                                        .map_or("", |s| s.name.as_ref());
                                    maps::lod_snow_color(name, base, snow, snows)
                                };
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
        // Persist each newly sampled biome with its waypoint. A known biome is
        // only refreshed when its chunk arrives; an unknown one is retried
        // until the chunk is loaded. Only a change reaches the settings
        // worker, so an unchanged sample writes nothing.
        let arrived = std::mem::take(&mut state.biome_chunks);
        for index in 0..state.waypoints.len() {
            let (world, dimension, x, y, z, current) = {
                let waypoint = &state.waypoints[index];
                (
                    waypoint.world.clone(),
                    waypoint.dimension,
                    waypoint.x,
                    waypoint.y,
                    waypoint.z,
                    waypoint.biome.clone(),
                )
            };
            if current.is_some() && !arrived.contains(&(x.div_euclid(16), z.div_euclid(16))) {
                continue;
            }
            let Some(biome) = known_biome(game, dimension, [x, y, z]) else {
                continue;
            };
            if current.as_deref() == Some(biome.as_str()) {
                continue;
            }
            state.waypoints[index].biome = Some(biome.clone());
            controller.update_waypoint_biome(&world, dimension, x, z, &biome);
        }
        let cx = (player.x.floor() as i32).div_euclid(16);
        let cz = (player.z.floor() as i32).div_euclid(16);
        let _timing = runtime::profiling::measure("chunk-collection");
        let revision = game.world.chunks.membership_revision();
        if state.chunk_revision != revision {
            if let Some(deltas) = game.world.chunks.deltas_since(state.chunk_revision) {
                use horizon_client::chunk_map::ChunkMembershipDelta;
                for (_, delta) in deltas {
                    match delta {
                        ChunkMembershipDelta::Added(pos) => {
                            state.chunk_keys.insert(pos);
                        }
                        ChunkMembershipDelta::Removed(pos) => {
                            state.chunk_keys.remove(&pos);
                            state.seen.remove(&pos);
                        }
                        ChunkMembershipDelta::Reset => {
                            unreachable!("reset requires membership rebuild")
                        }
                    }
                }
            } else {
                let keys: BTreeSet<_> = game.world.chunks.keys().copied().collect();
                state.seen.retain(|pos, _| keys.contains(pos));
                state.chunk_keys = keys;
            }
            state.chunk_revision = revision;
        }
        let requested = controller.maps.requested();
        let anchor = requested
            .as_ref()
            .filter(|v| v.dimension == dimension)
            .map_or([cx, cz], |v| {
                v.center.map(|v| (v.floor() as i32).div_euclid(16))
            });
        let mut positions: BTreeSet<_> = arrived.iter().copied().take(64).collect();
        for z in -4..=4 {
            for x in -4..=4 {
                positions.insert((anchor[0].saturating_add(x), anchor[1].saturating_add(z)));
            }
        }
        use std::ops::Bound::{Excluded, Unbounded};
        let scan: Vec<_> = state
            .chunk_keys
            .range((state.chunk_cursor.map_or(Unbounded, Excluded), Unbounded))
            .take(64)
            .copied()
            .collect();
        state.chunk_cursor = if scan.len() == 64 {
            scan.last().copied()
        } else {
            None
        };
        positions.extend(scan);
        let mut chunks: Vec<_> = positions
            .into_iter()
            .filter_map(|pos| {
                game.world
                    .chunks
                    .get(&pos)
                    .map(|chunk| (pos, Arc::downgrade(chunk)))
            })
            .collect();
        chunks.sort_by_key(|(pos, _)| {
            (
                !requested
                    .as_ref()
                    .is_some_and(|v| v.demands(dimension, layer, [pos.0, pos.1])),
                i64::from(pos.0 - cx).pow(2) + i64::from(pos.1 - cz).pow(2),
            )
        });
        let mut sent = 0;
        let mut attempted = 0;
        let collection_start = Instant::now();
        for (pos, weak) in chunks {
            if attempted > 0 && collection_start.elapsed() >= Duration::from_micros(800) {
                break;
            }
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
            attempted += 1;
            let snapshot = {
                let _sampling = runtime::profiling::measure("snapshot-collection");
                game.map_chunks([pos.0 - 1, pos.1 - 1], [pos.0 + 1, pos.1 + 1])
            };
            let Ok(snapshot) = snapshot else {
                continue;
            };
            let resources = Arc::clone(&game.res);
            let biome_palette = game
                .world_biomes
                .as_ref()
                .unwrap_or_else(|| game.res.biomes())
                .clone();
            let leaves = state.leaves.as_ref().map(|(_, leaves)| Arc::clone(leaves));
            let accepted = controller.maps.submit(
                state.session,
                dimension,
                layer,
                [pos.0, pos.1],
                move || {
                    let book = resources.facts();
                    let minimum = snapshot.height().start;
                    let mut names = BTreeMap::<u16, Arc<str>>::new();
                    maps::sample_with_biomes(
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
                            let leaf = leaves
                                .as_ref()
                                .and_then(|leaves| leaves.states.get(&id.0))
                                .copied();
                            let needs_grass = leaf.is_some_and(|leaf| leaf.tint == LeafTint::Grass)
                                || maps::needs_grass_tint(state.name.as_ref());
                            let needs_foliage = leaf
                                .is_some_and(|leaf| leaf.tint == LeafTint::Foliage)
                                || maps::needs_foliage_tint(state.name.as_ref());
                            let blend = blend_biome_tints(
                                &biome_palette,
                                [x, z],
                                BIOME_BLEND_RADIUS,
                                BIOME_BLEND_STEP,
                                needs_grass,
                                needs_foliage,
                                water,
                                |wx, wz| snapshot.biome([wx, y, wz]).map(|biome| biome.0),
                            );
                            let rgb = if water {
                                blend
                                    .water
                                    .unwrap_or_else(|| if base == 0 { 0x3f76e4 } else { base })
                            } else {
                                leaf.and_then(|leaf| leaf.map_color(blend.grass, blend.foliage))
                                    .unwrap_or_else(|| {
                                        plant_color(state.name.as_ref()).unwrap_or_else(|| {
                                            maps::block_map_color(
                                                state.name.as_ref(),
                                                base,
                                                blend.grass,
                                                blend.foliage,
                                            )
                                        })
                                    })
                            };
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
                        |at| {
                            let id = snapshot.biome(at)?.0;
                            let info = biome_palette.list.get(id as usize)?;
                            Some(
                                names
                                    .entry(id)
                                    .or_insert_with(|| Arc::from(info.name.as_str()))
                                    .clone(),
                            )
                        },
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
        let viewport = HudBatch::new(
            [renderer.config.width, renderer.config.height],
            game.settings.gui_scale_inputs(),
        );
        let rect = rectangle(viewport.width, viewport.height);
        let selected_layer = state
            .layers
            .get(&selected)
            .copied()
            .unwrap_or(Layer::Surface);
        let view = if large {
            large_camera_view(
                state.session,
                selected,
                selected_layer,
                &state.camera,
                rect,
                viewport.scale,
            )
        } else {
            let size = settings
                .fitted_size(viewport.width, viewport.height)
                .unwrap_or(128.0);
            let texture = (size * 4.0).round().clamp(64.0, 1024.0) as usize;
            View {
                session: state.session,
                dimension: selected,
                layer: selected_layer,
                center: state.camera.center,
                span: [f64::from(settings.blocks_per_pixel) * 128.0; 2],
                width: texture,
                height: texture,
                angle: if settings.rotate_with_player && selected == dimension {
                    (f64::from(player.yaw) + 180.0).to_radians()
                } else {
                    0.0
                },
                round: settings.round,
                large: false,
            }
        };
        let target = camera_target(&view, &state.camera);
        controller.maps.request_ahead(view.clone(), target);
        current_view = Some(view);
    }
    if let Some(image) = controller.maps.frame()
        && image.view.session == state.session
        && image.view.dimension == selected
        && image.view.large == large
    {
        if current_view
            .as_ref()
            .is_some_and(|view| view.layer != image.view.layer)
        {
            return;
        }
        // Camera movement is independent of worker sampling/upload latency.
        // Pointer queries and overlays use this exact same displayed view.
        let next = if let (Some(atlas), Some(view)) = (&image.atlas, current_view.clone()) {
            let view = complete_map_view(view, &image.view);
            Arc::new(maps::Frame {
                raster_source: None,
                serial: image.serial,
                view,
                pixels: Vec::new(),
                known: image.known,
                atlas: Some(atlas.clone()),
            })
        } else if let Some(view) = current_view.filter(|view| view.large) {
            Arc::new(maps::Frame {
                raster_source: Some(image.clone()),
                serial: image.serial,
                view: complete_map_view(view, &image.view),
                pixels: Vec::new(),
                known: image.known,
                atlas: None,
            })
        } else {
            image
        };
        let paged = next.atlas.as_ref().is_some_and(|a| !a.pages.is_empty());
        sync_page_images(&mut state.page_images, next.atlas.as_deref());
        let previous = if next.atlas.is_some() {
            state.frame.as_ref().and_then(|f| f.atlas.clone())
        } else {
            None
        };
        maps::present_frame(&mut state.frame, next, |width, height, pixels| {
            if paged {
                true
            } else if let Some(previous) = &previous {
                upload_terrain_patches(game, previous, width, height, pixels)
            } else {
                game.mod_image(width, height, pixels)
            }
        });
    }
    refresh_waypoint_labels(&mut state, controller.settings().waypoint_letters);
}

fn minimap_coordinate_lines(game: &Game<'_>, settings: runtime::minimap::Settings) -> Vec<String> {
    game.native_player().map_or_else(Vec::new, |player| {
        let dimension = match player.dimension {
            "minecraft:overworld" => Dimension::Overworld,
            "minecraft:the_nether" => Dimension::Nether,
            _ => return Vec::new(),
        };
        runtime::coordinate_lines(settings, dimension, [player.x, player.y, player.z])
    })
}

fn minimap_footer_height(lines: usize) -> f32 {
    if lines == 0 {
        0.
    } else {
        lines as f32 * 12. + 2.
    }
}

/// Fixed map slot, including coordinates and the gap before effect rows.
/// Use the same visibility and size checks as the actual HUD draw.
pub(super) fn top_right_map_height(game: &Game<'_>, width: f32, height: f32) -> Option<f32> {
    if !game.native_window()
        || !game.map_on(runtime::MOD_ID)
        || game.native_player().is_none()
        || game.mod_screen().is_some()
        || game.native_minimap_now().is_some()
        || !game.native_corner_shown(Corner::TopRight)
    {
        return None;
    }
    let settings = runtime::initialize(game.native_data_dir(runtime::MOD_ID)).settings();
    if !settings.enabled || settings.corner != runtime::minimap::Corner::TopRight {
        return None;
    }
    let size = settings.fitted_size(width, height)?;
    Some(size + minimap_footer_height(minimap_coordinate_lines(game, settings).len()) + 4.)
}

pub(super) fn hud(game: &Game<'_>, batch: &mut HudBatch, corners: &mut Corners) {
    apply_sdk_lifecycle(game);
    if !game.native_window()
        || !game.map_on(runtime::MOD_ID)
        || game.native_player().is_none()
        || game.mod_screen().is_some()
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
                let lines = minimap_coordinate_lines(game, settings);
                let footer = minimap_footer_height(lines.len());
                let width = lines
                    .iter()
                    .map(|line| game.map_text_width(line))
                    .fold(size, f32::max);
                // The top-right map owns a fixed slot above status effects.
                // effect_rows_height reserves that slot for other HUD panels.
                let (x, y) = if corner == Corner::TopRight {
                    Corners::new(batch.width, batch.height, 0.).place(corner, width, size + footer)
                } else {
                    corners.place(corner, width, size + footer)
                };
                let map_x = if matches!(corner, Corner::TopRight | Corner::BottomRight) {
                    x + width - size
                } else {
                    x
                };
                draw(game, batch, &state, [map_x, y, size, size], false, None);
                for (index, line) in lines.iter().enumerate() {
                    game.map_text(
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
}

pub(super) fn gui_render_post(
    game: &Game<'_>,
    batch: &mut HudBatch,
    gui_hidden: bool,
    focused: bool,
) {
    apply_sdk_lifecycle(game);
    if gui_hidden
        || !focused
        || !game.native_window()
        || !game.map_on(runtime::MOD_ID)
        || game.mod_screen().is_some()
        || game.screen != crate::game::Screen::Playing
        || game.native_player().is_none()
    {
        return;
    }
    let state = state();
    let settings = runtime::initialize(game.native_data_dir(runtime::MOD_ID)).settings();
    draw_world_waypoints(game, batch, &state, &settings);
}

/// The eight compass points as world-space unit steps (`dx`, `dz`): north is
/// `-Z`, east is `+X`. `draw_directions` rotates them by the map's angle.
const DIRECTION_STEPS: [(f64, f64); 8] = [
    (0.0, -1.0),  // N
    (1.0, -1.0),  // NE
    (1.0, 0.0),   // E
    (1.0, 1.0),   // SE
    (0.0, 1.0),   // S
    (-1.0, 1.0),  // SW
    (-1.0, 0.0),  // W
    (-1.0, -1.0), // NW
];

fn direction_label(style: runtime::minimap::DirectionStyle, index: usize) -> &'static str {
    match style {
        runtime::minimap::DirectionStyle::Compass => match index {
            0 => tr("N", "С", "N", "N", "N", "北"),
            1 => tr("NE", "СВ", "NE", "NE", "NO", "东北"),
            2 => tr("E", "В", "E", "E", "O", "东"),
            3 => tr("SE", "ЮВ", "SE", "SE", "SO", "东南"),
            4 => tr("S", "Ю", "S", "S", "S", "南"),
            5 => tr("SW", "ЮЗ", "SO", "SO", "SW", "西南"),
            6 => tr("W", "З", "O", "O", "W", "西"),
            7 => tr("NW", "СЗ", "NO", "NO", "NW", "西北"),
            _ => "",
        },
        runtime::minimap::DirectionStyle::Axes => match index {
            0 => "Z-",
            1 => "X+Z-",
            2 => "X+",
            3 => "X+Z+",
            4 => "Z+",
            5 => "X-Z+",
            6 => "X-",
            7 => "X-Z-",
            _ => "",
        },
    }
}

/// Cardinal letters at the inner edge of the map. They rotate with the map so
/// the north marker always points at real north.
fn draw_directions(
    game: &Game<'_>,
    batch: &mut HudBatch,
    view: &View,
    rect: [f32; 4],
    round: bool,
    large: bool,
    settings: &runtime::minimap::Settings,
) {
    if !settings.directions {
        return;
    }
    let (s, c) = view.angle.sin_cos();
    let cx = f64::from(rect[0]) + f64::from(rect[2]) / 2.0;
    let cy = f64::from(rect[1]) + f64::from(rect[3]) / 2.0;
    // The large map's dimension tabs and coordinate footer float at the top and
    // bottom; keep the letters clear of them there.
    let (inset, top, bottom) = if large {
        (12.0, 36.0, 52.0)
    } else {
        (8.0, 8.0, 8.0)
    };
    let radius = f64::from(rect[2].min(rect[3])) / 2.0 - f64::from(inset);
    let half_width = f64::from(rect[2]) / 2.0 - f64::from(inset);
    let half_top = f64::from(rect[3]) / 2.0 - f64::from(top);
    let half_bottom = f64::from(rect[3]) / 2.0 - f64::from(bottom);
    for (index, (dx, dz)) in DIRECTION_STEPS.iter().copied().enumerate() {
        // Screen direction of a world step, rotating with the map's angle.
        let sx = dx * c + dz * s;
        let sy = -dx * s + dz * c;
        let (x, y) = if round {
            let length = sx.hypot(sy);
            if length <= f64::EPSILON {
                continue;
            }
            (cx + sx / length * radius, cy + sy / length * radius)
        } else {
            let mut t = f64::INFINITY;
            if sx.abs() > 1e-6 {
                t = t.min(half_width / sx.abs());
            }
            if sy < 0.0 {
                t = t.min(half_top / sy.abs());
            } else if sy > 0.0 {
                t = t.min(half_bottom / sy.abs());
            }
            if !t.is_finite() {
                continue;
            }
            (cx + sx * t, cy + sy * t)
        };
        let label = direction_label(settings.direction_style, index);
        let width = game.map_text_width(label);
        let lx = x as f32 - width / 2.0;
        let ly = y as f32 - 4.0;
        game.native_fill(batch, [lx - 2.0, ly - 2.0, width + 4.0, 12.0], 0x90000000);
        game.map_text(batch, label, [lx, ly], 0xffffffff, true);
    }
}

fn upload_terrain_patches(
    game: &mut Game<'_>,
    previous: &maps::TerrainAtlas,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> bool {
    let _timing = runtime::profiling::measure("upload");
    if previous.width != width as usize || previous.height != height as usize {
        return game.mod_image(width, height, pixels);
    }
    let Some(sprite) = game
        .gui
        .atlas
        .sprites
        .get(horizon_client::hud::MOD_IMAGE_SPRITE)
        .copied()
    else {
        return false;
    };
    if width > sprite.w || height > sprite.h || pixels.len() != width as usize * height as usize * 4
    {
        return false;
    }
    let origin = [
        (sprite.u0 * game.gui.atlas.width as f32).round() as u32,
        (sprite.v0 * game.gui.atlas.height as f32).round() as u32,
    ];
    for [x, y, w, h] in
        maps::image_patch_regions(&previous.pixels, pixels, width as usize, height as usize)
    {
        runtime::profiling::record("texture-transfer", Duration::ZERO, 1, (w * h * 4) as u64);
        let mut rgba = Vec::with_capacity(w * h * 4);
        for row in y..y + h {
            let start = (row * width as usize + x) * 4;
            rgba.extend_from_slice(&pixels[start..start + w * 4]);
        }
        game.map_patches.push(horizon_client::font::Patch {
            x: origin[0] + x as u32,
            y: origin[1] + y as u32,
            width: w as u32,
            height: h as u32,
            rgba,
        });
    }
    true
}

fn draw_terrain_polygon(
    game: &Game<'_>,
    batch: &mut HudBatch,
    view: &View,
    rect: [f32; 4],
    bounds: [i64; 4],
    uv: [f32; 4],
) -> bool {
    let points = maps::terrain_polygon(view, bounds, uv);
    let Some(sprite) = game
        .gui
        .atlas
        .sprites
        .get(horizon_client::hud::MOD_IMAGE_SPRITE)
    else {
        return false;
    };
    let pos = |p: [f64; 4]| {
        [
            rect[0] + (p[0] as f32 + 0.5) * rect[2],
            rect[1] + (p[1] as f32 + 0.5) * rect[3],
        ]
    };
    let texture = |p: [f64; 4]| {
        [
            sprite.u0 + p[2] as f32 / sprite.w as f32 * (sprite.u1 - sprite.u0),
            sprite.v0 + p[3] as f32 / sprite.h as f32 * (sprite.v1 - sprite.v0),
        ]
    };
    for i in 1..points.len().saturating_sub(1) {
        let p = [points[0], points[i], points[i + 1], points[i + 1]];
        batch.quad(p.map(pos), p.map(texture), u32::MAX);
    }
    points.len() >= 3
}

fn sync_page_images(images: &mut PageImages, atlas: Option<&maps::TerrainAtlas>) {
    let Some(atlas) = atlas else {
        images.clear();
        return;
    };
    if images.len() == atlas.pages.len()
        && images
            .iter()
            .zip(&atlas.pages)
            .all(|((old, _), page)| Arc::ptr_eq(old, page))
    {
        return;
    }
    let _timing = runtime::profiling::measure("page-upload");
    *images = atlas
        .pages
        .iter()
        .map(|page| {
            let image = images
                .iter()
                .find(|(old, _)| Arc::ptr_eq(old, page))
                .map(|(_, image)| image.clone())
                .unwrap_or_else(|| {
                    runtime::profiling::record(
                        "texture-transfer",
                        Duration::ZERO,
                        1,
                        page.pixels.len() as u64,
                    );
                    super::minimap_pages::create(
                        page.width as u32,
                        page.height as u32,
                        page.pixels.clone(),
                    )
                });
            (page.clone(), image)
        })
        .collect();
}

fn draw_paged_terrain(
    batch: &mut HudBatch,
    images: &PageImages,
    view: &View,
    atlas: &maps::TerrainAtlas,
    rect: [f32; 4],
) -> bool {
    let mut visible = false;
    // Keep the padded/predicted page set resident on the GPU, including pages
    // just behind the camera. Horizon drops picture textures absent from a batch.
    for (_, image) in images {
        if !batch.pictures.iter().any(|old| Arc::ptr_eq(old, image)) {
            batch.pictures.push(image.clone());
        }
    }
    let mut paint = |bounds: [i64; 4], mut uv: [f32; 4]| {
        let page = uv[1] as usize / 64;
        let Some((_, image)) = images.get(page) else {
            return;
        };
        uv[1] -= (page * 64) as f32;
        let mut points = maps::terrain_polygon(view, bounds, uv);
        for p in &mut points {
            p[0] = f64::from(rect[0]) + (p[0] + 0.5) * f64::from(rect[2]);
            p[1] = f64::from(rect[1]) + (p[1] + 0.5) * f64::from(rect[3]);
        }
        visible |= points.len() >= 3;
        super::minimap_pages::paint(batch, image, &points);
    };
    for coarse in &atlas.coarse {
        let bounds = coarse.key.bounds();
        for part in &coarse.fragments {
            paint(
                *part,
                [
                    coarse.uv_origin[0] as f32
                        + ((part[0] - bounds[0]) as f64 * 16. / bounds[2] as f64) as f32,
                    coarse.uv_origin[1] as f32
                        + ((part[1] - bounds[1]) as f64 * 16. / bounds[3] as f64) as f32,
                    (part[2] as f64 * 16. / bounds[2] as f64) as f32,
                    (part[3] as f64 * 16. / bounds[3] as f64) as f32,
                ],
            );
        }
    }
    for &i in &atlas.draw_order {
        let chunk = &atlas.chunks[i];
        let slot = atlas.slots[i];
        paint(
            [i64::from(chunk[0]) * 16, i64::from(chunk[1]) * 16, 16, 16],
            [(slot % 64 * 16) as f32, (slot / 64 * 16) as f32, 16., 16.],
        );
    }
    visible
}

struct MapScene<'a> {
    rect: [f32; 4],
    frame: &'a maps::Frame,
    images: &'a PageImages,
    hover: Option<[i32; 2]>,
    waypoints: &'a [Waypoint],
    labels: &'a [String],
    marker: Option<PlayerMarker>,
    settings: &'a runtime::minimap::Settings,
}

fn draw_map_scene(
    game: &Game<'_>,
    batch: &mut HudBatch,
    scene: MapScene<'_>,
    draw_live_overlays: impl FnOnce(&mut HudBatch),
) -> bool {
    let frame = scene.frame;
    let view = &frame.view;
    let rect = scene.rect;
    let mut terrain_visible = frame.known > 0;
    if let Some(atlas) = &frame.atlas {
        terrain_visible = false;
        if !atlas.pages.is_empty() {
            terrain_visible = draw_paged_terrain(batch, scene.images, view, atlas, rect);
        } else {
            for coarse in &atlas.coarse {
                for part in &coarse.fragments {
                    if view.large {
                        if let Some((src, dst)) = atlas.coarse_quad(view, rect, coarse, *part) {
                            game.map_sprite(batch, horizon_client::hud::MOD_IMAGE_SPRITE, src, dst);
                            terrain_visible = true;
                        }
                    } else {
                        let bounds = coarse.key.bounds();
                        let uv = [
                            coarse.uv_origin[0] as f32
                                + ((part[0] - bounds[0]) as f64 * 16.0 / bounds[2] as f64) as f32,
                            coarse.uv_origin[1] as f32
                                + ((part[1] - bounds[1]) as f64 * 16.0 / bounds[3] as f64) as f32,
                            (part[2] as f64 * 16.0 / bounds[2] as f64) as f32,
                            (part[3] as f64 * 16.0 / bounds[3] as f64) as f32,
                        ];
                        terrain_visible |= draw_terrain_polygon(game, batch, view, rect, *part, uv);
                    }
                }
            }
            for (slot, chunk) in atlas.chunks.iter().enumerate() {
                if view.large {
                    if let Some((src, dst)) = atlas.quad(view, rect, *chunk, slot) {
                        game.map_sprite(batch, horizon_client::hud::MOD_IMAGE_SPRITE, src, dst);
                        terrain_visible = true;
                    }
                } else {
                    let columns = atlas.width / 16;
                    let slot = atlas.slots[slot];
                    terrain_visible |= draw_terrain_polygon(
                        game,
                        batch,
                        view,
                        rect,
                        [i64::from(chunk[0]) * 16, i64::from(chunk[1]) * 16, 16, 16],
                        [
                            (slot % columns * 16) as f32,
                            (slot / columns * 16) as f32,
                            16.,
                            16.,
                        ],
                    );
                }
            }
        }
    } else if let Some(source) = &frame.raster_source {
        if let Some((src, dst)) = maps::raster_quad(&source.view, view, rect) {
            game.map_sprite(batch, horizon_client::hud::MOD_IMAGE_SPRITE, src, dst);
        } else {
            terrain_visible = false;
        }
    } else if frame.known > 0 {
        game.map_sprite(
            batch,
            horizon_client::hud::MOD_IMAGE_SPRITE,
            [0.0, 0.0, frame.view.width as f32, frame.view.height as f32],
            rect,
        );
    }
    draw_live_overlays(batch);
    if let Some(block) = scene.hover {
        highlight(game, batch, view, rect, block);
    }
    for (index, waypoint) in scene
        .waypoints
        .iter()
        .enumerate()
        .filter(|(_, waypoint)| waypoint.dimension == view.dimension)
    {
        let fallback;
        let label = if let Some(label) = scene.labels.get(index) {
            label
        } else {
            fallback = first_letter(&waypoint.name).to_string();
            &fallback
        };
        draw_waypoint(
            game,
            batch,
            view,
            rect,
            waypoint,
            label,
            if view.large { 11. } else { 8. },
        );
    }
    if let Some(marker) = scene.marker {
        draw_marker(game, batch, view, rect, marker);
    }
    draw_directions(
        game,
        batch,
        view,
        rect,
        view.round,
        view.large,
        scene.settings,
    );
    terrain_visible
}

fn draw(
    game: &Game<'_>,
    batch: &mut HudBatch,
    state: &State,
    rect: [f32; 4],
    large: bool,
    mouse: Option<[f32; 2]>,
) {
    let _timing = runtime::profiling::measure("hud-map");
    let settings = runtime::initialize(game.native_data_dir(runtime::MOD_ID)).settings();
    let round = !large && settings.round;
    if !round {
        game.native_fill(batch, rect, 0xef181b20);
    }
    let valid = state.frame.as_ref().filter(|f| {
        f.view.dimension == state.selected
            && f.view.large == large
            && f.view.session == state.session
    });
    if let Some(frame) = valid {
        if let Some(mouse) = mouse {
            let at = frame.view.block_at(
                f64::from((mouse[0] - rect[0]) / rect[2]) * frame.view.width as f64,
                f64::from((mouse[1] - rect[1]) / rect[3]) * frame.view.height as f64,
            );
            runtime::initialize(game.native_data_dir(runtime::MOD_ID))
                .maps
                .inspect(
                    frame.view.session,
                    frame.view.dimension,
                    frame.view.layer,
                    at,
                );
        }
        let hover = if large {
            state
                .menu
                .map(|menu| menu.block)
                .or_else(|| mouse.and_then(|mouse| hovered_block(&frame.view, rect, mouse)))
        } else {
            None
        };
        let marker = game.native_player().map(|player| PlayerMarker {
            at: state.marker,
            yaw: Some(f64::from(player.yaw)),
            colors: if Dimension::from_id(player.dimension) == frame.view.dimension {
                (0xfff0f4fa, 0xffff5050)
            } else {
                (0xc0f0f4fa, 0xc060a0ff)
            },
        });
        let terrain_visible = draw_map_scene(
            game,
            batch,
            MapScene {
                rect,
                frame,
                images: &state.page_images,
                hover,
                waypoints: &state.waypoints,
                labels: &state.waypoint_labels,
                marker,
                settings: &settings,
            },
            |batch| {
                draw_border(game, batch, &frame.view, rect, mouse);
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
                    if let Some(menu) = state.menu {
                        let (chunk, region) = maps::chunk_region(menu.block);
                        draw_world_box(
                            game,
                            batch,
                            &frame.view,
                            rect,
                            [
                                f64::from(chunk[0] * 16),
                                f64::from(chunk[1] * 16),
                                f64::from(chunk[0] * 16 + 16),
                                f64::from(chunk[1] * 16 + 16),
                            ],
                            0xffffd45c,
                        );
                        draw_world_box(
                            game,
                            batch,
                            &frame.view,
                            rect,
                            [
                                f64::from(region[0] * 512),
                                f64::from(region[1] * 512),
                                f64::from(region[0] * 512 + 512),
                                f64::from(region[1] * 512 + 512),
                            ],
                            0xff58c7ff,
                        );
                    }
                }
            },
        );
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
            "无数据",
        )
    } else {
        tr(
            "Preparing map…",
            "Подготовка карты…",
            "Préparation de la carte…",
            "Preparando mapa…",
            "Karte wird vorbereitet…",
            "正在准备地图…",
        )
    };
    game.map_text(
        batch,
        text,
        [
            rect[0] + (rect[2] - game.map_text_width(text)) / 2.0,
            rect[1] + rect[3] / 2.0 - 4.0,
        ],
        0xcccccc,
        true,
    );
}

fn tab(width: f32, index: usize) -> [f32; 4] {
    let w = ((width - 16.0) / 4.0).max(1.0);
    [8.0 + w * index as f32, 8.0, (w - 2.0).max(1.0), 24.0]
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
            "客户端详细区块（已缓存）",
        )
    } else if lod.is_some() {
        tr(
            "LOD — approximate data",
            "LOD — приблизительные данные",
            "LOD — données approximatives",
            "LOD — datos aproximados",
            "LOD — ungefähre Daten",
            "LOD（近似数据）",
        )
    } else {
        tr(
            "No data for this layer",
            "Нет данных для этого слоя",
            "Aucune donnée pour cette couche",
            "Sin datos para esta capa",
            "Keine Daten für diese Ebene",
            "此图层无数据",
        )
    };
    let layer_text = match layer {
        Layer::Surface => tr(
            "Surface",
            "Поверхность",
            "Surface",
            "Superficie",
            "Oberfläche",
            "地表",
        )
        .to_owned(),
        Layer::Below(y) => localized_format!(
            "Below Y {y}",
            "Ниже Y {y}",
            "Sous Y {y}",
            "Por debajo de Y {y}",
            "Unter Y {y}",
            "Y {y} 以下"
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
    ];
    if let (Some(y), Some(fluid)) = (menu.height, menu.fluid) {
        let kind = match fluid.kind {
            FluidKind::Water => tr("Water", "Вода", "Eau", "Agua", "Wasser", "水"),
            FluidKind::Lava => tr("Lava", "Лава", "Lave", "Lava", "Lava", "熔岩"),
        };
        lines.push((
            localized_format!(
                "{}: surface Y {y}, bottom Y {}",
                "{}: поверхность Y {y}, дно Y {}",
                "{} : surface Y {y}, fond Y {}",
                "{}: superficie Y {y}, fondo Y {}",
                "{}: Oberfläche Y {y}, Grund Y {}",
                "{}：表面 Y {y}，底部 Y {}",
                kind,
                fluid.bottom
            ),
            normal,
        ));
    }
    lines.extend([
        (
            localized_format!(
                "Chunk: {} / {}",
                "Чанк: {} / {}",
                "Chunk : {} / {}",
                "Chunk: {} / {}",
                "Chunk: {} / {}",
                "区块：{} / {}",
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
    ]);
    if let Some(key) = lod {
        let stride = 1_i64 << key.level;
        let level = key.level;
        lines.push((
            localized_format!(
                "LOD quality: level {level} (1 sample per {stride}×{stride} blocks)",
                "Качество LOD: уровень {level} (1 отсчёт на {stride}×{stride} блоков)",
                "Qualité LOD : niveau {level} (1 échantillon pour {stride}×{stride} blocs)",
                "Calidad LOD: nivel {level} (1 muestra por {stride}×{stride} bloques)",
                "LOD-Qualität: Stufe {level} (1 Abtastwert je {stride}×{stride} Blöcke)",
                "LOD 精度：{level} 级（每 {stride}×{stride} 方块采样一次）",
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
                tr("Biome: —", "биом: —", "Biome : —", "Bioma: —", "Biom: —", "生物群系：—").into()
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
                    "区域：{} / {}（32×32 区块）",
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
                tr("Region", "Регион", "Région", "Región", "Region", "区域")
            } else {
                tr("Chunk", "Чанк", "Chunk", "Chunk", "Chunk", "区块")
            };
            let status = if d > 0 {
                tr(
                    "mixed coverage: detailed + LOD",
                    "смешанное покрытие: подробное + LOD",
                    "couverture mixte : détaillée + LOD",
                    "cobertura mixta: detallada + LOD",
                    "gemischte Abdeckung: detailliert + LOD",
                    "混合覆盖：详细区块 + LOD",
                )
            } else {
                tr(
                    "LOD coverage — approximate",
                    "LOD-покрытие — приблизительно",
                    "couverture LOD — approximative",
                    "cobertura LOD — aproximada",
                    "LOD-Abdeckung — ungefähr",
                    "LOD 覆盖（近似）",
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
                        "地表 LOD 无法显示此地下图层",
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
                    "Detaillierte Chunks im Cache: {d}; LOD-Kacheln: {l}",
                    "已缓存详细区块：{d}，LOD 覆盖单元：{l}"
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
            "暂未记录访问历史",
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
            if !line.is_empty() && game.map_text_width(&next) > width {
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
    game.map_text(
        batch,
        tr(
            "Chunk / region",
            "Чанк / регион",
            "Chunk / région",
            "Chunk / región",
            "Chunk / Region",
            "区块 / 区域",
        ),
        [rect[0] + 10.0, rect[1] + 8.0],
        0xffffff,
        true,
    );
    let lines = information_lines(game, state, menu, rect[2] - 24.0);
    let visible = ((rect[3] - 56.0) / 12.0).max(1.0) as usize;
    let start = state
        .info_scroll
        .offset(lines.len().saturating_sub(visible));
    for (i, (text, color)) in lines.iter().skip(start).take(visible).enumerate() {
        game.map_text(
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
        ButtonIcon::Cancel,
        [
            rect[0] + rect[2] - 82.0,
            rect[1] + rect[3] - 24.0,
            72.0,
            18.0,
        ],
        tr("Close", "Закрыть", "Fermer", "Cerrar", "Schließen", "关闭"),
        game.cursor(),
        true,
    );
}

fn nether_coordinates(view: &View, block: [i32; 2]) -> Option<String> {
    let [x, _, z] = runtime::portal_coordinates(
        view.dimension,
        Dimension::Nether,
        [f64::from(block[0]), 0., f64::from(block[1])],
    )?;
    Some(format!("{}: X {x}  Z {z}", Dimension::Nether.label()))
}

fn map_footer(width: f32, height: f32) -> [f32; 4] {
    [
        8.0,
        (height - 47.0).max(0.0),
        (width - 16.0).max(1.0),
        39.0_f32.min(height),
    ]
}
fn over_map_controls(mouse: [f32; 2], width: f32, height: f32) -> bool {
    inside(mouse, cache_map_button(width))
        || inside(mouse, map_footer(width, height))
        || (0..4).any(|i| inside(mouse, tab(width, i)))
}
fn draw_dimension_tab(
    game: &Game<'_>,
    batch: &mut HudBatch,
    rect: [f32; 4],
    dimension: Dimension,
    selected: bool,
    available: bool,
) {
    button(
        game,
        batch,
        ButtonIcon::Plain,
        rect,
        "",
        game.cursor(),
        available,
    );
    if selected && available {
        game.native_fill(batch, rect, 0xe032624b);
    }
    dimension_icon(game, batch, dimension, [rect[0] + 4., rect[1] + 4.]);
    let label = fit_text(game, dimension.label(), rect[2] - 25.);
    game.map_text(
        batch,
        &label,
        [rect[0] + 24., rect[1] + 8.],
        if available { 0xffffff } else { 0x707880 },
        true,
    );
}

fn draw_map_screen(game: &Game<'_>, batch: &mut HudBatch, state: &State) {
    let mouse = game.cursor();
    let rect = rectangle(batch.width, batch.height);
    draw(game, batch, state, rect, true, Some(mouse));
    let mut coordinates = tr(
        "Cursor: —",
        "Курсор: —",
        "Curseur : —",
        "Cursor: —",
        "Mauszeiger: —",
        "光标：—",
    )
    .to_owned();
    let mut nether = tr(
        "Nether: —",
        "Незер: —",
        "Nether : —",
        "Nether: —",
        "Nether: —",
        "下界：—",
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
        let column = if state.dimension == frame.view.dimension {
            hovered_column(game, frame.view.layer, block)
        } else {
            match frame.view.layer {
                Layer::Below(y) => Some(ColumnInfo {
                    height: y,
                    fluid: None,
                }),
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
        let detailed = coverage.detailed_biome(frame.view.dimension, frame.view.layer, block);
        let height_approximate = column.is_none() && rough.is_some();
        let height = column
            .map(|column| column.height)
            .or_else(|| rough.as_ref().map(|p| p.height));
        let bottom = column.and_then(|column| column.fluid.map(|fluid| fluid.bottom));
        coordinates = match height {
            Some(y) => {
                let bottom = bottom.map_or_else(String::new, |bottom| {
                    localized_format!(
                        " (bottom {})",
                        " (дно {})",
                        " (fond {})",
                        " (fondo {})",
                        " (Grund {})",
                        "（底部 {}）",
                        bottom
                    )
                });
                format!(
                    "{}: X {}  Y {}{}{}  Z {}",
                    frame.view.dimension.label(),
                    block[0],
                    if height_approximate { "≈" } else { "" },
                    y,
                    bottom,
                    block[1]
                )
            }
            None => format!(
                "{}: X {}  Z {}",
                frame.view.dimension.label(),
                block[0],
                block[1]
            ),
        };
        if let Some(converted) = nether_coordinates(&frame.view, block) {
            nether = converted;
        }
        if state.dimension == frame.view.dimension {
            if let Some(y) = height.or_else(|| game.native_player().map(|p| p.y.floor() as i32)) {
                biome = known_biome(game, state.dimension, [block[0], y, block[1]]);
            }
        }
        if biome.is_none() {
            biome = detailed
                .as_ref()
                .or_else(|| rough.as_ref().and_then(|sample| sample.biome.as_ref()))
                .map(|name| name.to_string());
        }
        lod_warning = !coverage.detailed_at(frame.view.dimension, frame.view.layer, block)
            && coverage
                .lod_at(frame.view.dimension, frame.view.layer, block)
                .is_some();
        if let Some(waypoint) = waypoint_at_pointer(game, state, &frame.view, rect, mouse) {
            tooltip = Some(format!(
                "{}  ({} {} {})",
                waypoint.name, waypoint.x, waypoint.y, waypoint.z
            ));
        }
    }
    let settings = runtime::initialize(game.native_data_dir(runtime::MOD_ID)).settings();
    for (i, dimension) in Dimension::ALL.into_iter().enumerate() {
        draw_dimension_tab(
            game,
            batch,
            tab(batch.width, i),
            dimension,
            dimension == settings.world_dimension,
            true,
        );
    }
    let footer = map_footer(batch.width, batch.height);
    game.native_fill(batch, footer, 0xdf10151d);
    let coordinates = fit_text(game, &format!("{coordinates}   {nether}"), footer[2] - 12.0);
    game.map_text(
        batch,
        &coordinates,
        [footer[0] + 6.0, footer[1] + 5.0],
        0xffffff,
        true,
    );
    biome_icon(
        game,
        batch,
        biome.as_deref(),
        [footer[0] + 6.0, footer[1] + 18.0],
    );
    let mut label = biome.as_deref().map_or_else(
        || tr("Biome: —", "биом: —", "Biome : —", "Bioma: —", "Biom: —", "生物群系：—").to_owned(),
        localized_biome,
    );
    if lod_warning {
        label.push_str(tr(
            " | LOD — approximate",
            " | LOD — приблизительно",
            " | LOD — approximatif",
            " | LOD — aproximado",
            " | LOD — ungefähr",
            " | LOD（近似）",
        ));
    }
    game.map_text(
        batch,
        &label,
        [footer[0] + 26.0, footer[1] + 21.0],
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
    if state.info.is_none() && state.menu.is_none() {
        let controller = runtime::initialize(game.native_data_dir(runtime::MOD_ID));
        let attention = controller.maps.needs_upgrade(&state.world) || state.caches.attention;
        button(
            game,
            batch,
            ButtonIcon::Cache,
            cache_map_button(batch.width),
            if attention {
                tr(
                    "Update cache…",
                    "Обновить кеш…",
                    "Actualiser cache…",
                    "Actualizar caché…",
                    "Cache aktualisieren…",
                    "更新缓存…",
                )
            } else {
                tr(
                    "Map caches…",
                    "Кеши карт…",
                    "Caches…",
                    "Cachés…",
                    "Karten-Caches…",
                    "地图缓存…",
                )
            },
            mouse,
            true,
        );
    }
    if let Some(info) = state.info {
        draw_information(game, batch, state, info);
    }
}

fn cache_map_button(width: f32) -> [f32; 4] {
    [(width - 106.).max(6.), 36., 100., 18.]
}
fn cache_rows(height: f32) -> usize {
    (((height - 122.).max(0.) / 48.) as usize).clamp(1, 16)
}
fn cache_row(width: f32, index: usize) -> [f32; 4] {
    [12., 42. + index as f32 * 48., (width - 38.).max(1.), 46.]
}
fn cache_page_button(width: f32, forwards: bool) -> [f32; 4] {
    [width - if forwards { 122. } else { 150. }, 10., 24., 18.]
}
fn cache_scrollbar(width: f32, height: f32) -> [f32; 4] {
    [width - 20., 42., 8., cache_rows(height) as f32 * 48. - 2.]
}
fn cache_scroll_to(state: &mut State, width: f32, height: f32, mouse_y: f32, count: usize) {
    let max = count.saturating_sub(cache_rows(height));
    let track = cache_scrollbar(width, height);
    let bar = runtime::ListScrollbar::new(
        track[1],
        track[3],
        cache_rows(height),
        count,
        state.caches.scroll.offset(max),
    );
    let grab = state.caches.scrollbar_grab.unwrap_or(bar.thumb_height / 2.);
    state.caches.scroll.jump(bar.row_at(mouse_y, grab), max);
}

fn cache_footer(width: f32, height: f32, index: usize) -> [f32; 4] {
    let w = ((width - 36.) / 4.).max(1.);
    [12. + index as f32 * (w + 4.), height - 55., w, 18.]
}
fn cache_status(status: &runtime::container::Compatibility) -> &'static str {
    use runtime::container::Compatibility::*;
    match status {
        Current => tr(
            "Compatible",
            "Совместим",
            "Compatible",
            "Compatible",
            "Kompatibel",
            "兼容",
        ),
        RebuildImages => tr(
            "Update available: rebuild map images",
            "Доступно обновление: перестройте изображения карты",
            "Mise à jour : reconstruire les images",
            "Actualización: reconstruir imágenes",
            "Update: Kartenbilder neu aufbauen",
            "可更新地图图像",
        ),
        Older(_) => tr(
            "Old format: conversion is not available yet",
            "Старый формат: конвертация пока недоступна",
            "Ancien format : conversion indisponible",
            "Formato antiguo: conversión no disponible",
            "Altes Format: Konvertierung noch nicht verfügbar",
            "旧版格式，暂不支持转换",
        ),
        Newer(_) => tr(
            "Requires a newer mod version",
            "Нужна более новая версия мода",
            "Version plus récente du mod requise",
            "Requiere una versión más reciente del mod",
            "Neuere Mod-Version erforderlich",
            "需要更新的模组版本",
        ),
        Damaged => tr(
            "Header unreadable; file preserved",
            "Заголовок не читается; файл сохранён",
            "En-tête illisible ; fichier conservé",
            "Cabecera ilegible; archivo conservado",
            "Header unlesbar; Datei erhalten",
            "无法读取文件头，文件已保留",
        ),
    }
}
fn cache_kind(world: Option<&str>) -> &'static str {
    match world {
        Some(world) if world.starts_with("save:") => tr(
            "Local world",
            "Локальный мир",
            "Monde local",
            "Mundo local",
            "Lokale Welt",
            "本地世界",
        ),
        Some(world) if world.starts_with("server:") => {
            tr("Server", "Сервер", "Serveur", "Servidor", "Server", "服务器")
        }
        _ => tr(
            "Unknown world",
            "Неизвестный мир",
            "Monde inconnu",
            "Mundo desconocido",
            "Unbekannte Welt",
            "未知世界",
        ),
    }
}
fn cache_text(game: &Game<'_>, value: &str, width: f32) -> String {
    let mut text: String = value
        .chars()
        .filter(|c| !c.is_control())
        .take(160)
        .collect();
    if game.map_text_width(&text) <= width {
        return text;
    }
    while !text.is_empty() && game.map_text_width(&format!("{text}…")) > width {
        text.pop();
    }
    format!("{text}…")
}
fn cache_icon(bytes: &[u8]) -> Option<Arc<Vec<u8>>> {
    // Validate size before the PNG decoder allocates its output.
    if bytes.len() > 1024 * 1024
        || bytes.len() < 24
        || &bytes[..8] != b"\x89PNG\r\n\x1a\n"
        || &bytes[12..16] != b"IHDR"
        || u32::from_be_bytes(bytes[16..20].try_into().ok()?) != 64
        || u32::from_be_bytes(bytes[20..24].try_into().ok()?) != 64
    {
        return None;
    }
    horizon_client::hud::decode_bytes(bytes)
        .ok()
        .filter(|(w, h, p)| *w == 64 && *h == 64 && p.len() == 64 * 64 * 4)
        .map(|(_, _, p)| Arc::new(p))
}
fn refresh_cache_catalog(game: &Game<'_>) {
    let controller = runtime::initialize(game.native_data_dir(runtime::MOD_ID));
    controller.caches.refresh(|rows| {
        use std::io::Read;
        let saves = std::env::current_dir()?.join("saves");
        let (servers, hidden) =
            horizon_protocol::servers::load_all(&horizon_client::multiplayer::servers_file(&saves));
        let servers: Vec<_> = servers.into_iter().chain(hidden).collect();
        for row in rows {
            let Some(world) = row.world.as_deref() else {
                continue;
            };
            if let Some(folder) = world.strip_prefix("save:") {
                let mut parts = std::path::Path::new(folder).components();
                if !matches!(parts.next(), Some(std::path::Component::Normal(_)))
                    || parts.next().is_some()
                    || folder.contains('\\')
                {
                    continue;
                }
                let root = saves.join(folder);
                if let Some(level) = horizon_client::save::Level::read_summary(&root) {
                    row.name = level.name;
                }
                if let Ok(file) = std::fs::File::open(root.join("icon.png")) {
                    let mut bytes = Vec::new();
                    if file.take(1024 * 1024 + 1).read_to_end(&mut bytes).is_ok() {
                        row.icon = cache_icon(&bytes);
                    }
                }
            } else if let Some(address) = world.strip_prefix("server:")
                && let Some(server) = servers.iter().find(|s| s.ip == address)
            {
                if !server.name.is_empty() {
                    row.name = server.name.clone();
                }
                row.icon = server
                    .icon
                    .as_deref()
                    .filter(|s| s.len() <= 1400000)
                    .and_then(horizon_client::skins::base64_decode)
                    .and_then(|png| cache_icon(&png));
            }
        }
        Ok(())
    });
}
fn cache_title() -> &'static str {
    tr(
        "Map caches",
        "Кеши карт",
        "Caches des cartes",
        "Cachés de mapas",
        "Karten-Caches",
        "地图缓存",
    )
}

fn title_cache_button(width: f32) -> [f32; 4] {
    [12., 12., (width - 24.).clamp(1., 124.), 18.]
}

fn close_cache_manager(state: &mut State) {
    runtime::set_text_input(false);
    state.caches.title_open = false;
    state.caches.preview = None;
    state.caches.confirm = None;
    state.caches.confirm_delete = None;
    state.set_mode(Mode::Map);
}

fn is_active_cache(state: &State, entry: &runtime::cache_manager::Entry) -> bool {
    !state.world.is_empty()
        && (entry.world.as_ref() == Some(&state.world)
            || runtime::container::world_path(std::path::Path::new(""), &state.world)
                .file_name()
                .is_some_and(|name| name == entry.file.as_str()))
}

impl CachePreview {
    fn layers(&self) -> &[(Layer, [f64; 2])] {
        self.dimensions
            .iter()
            .find(|d| d.dimension == self.dimension)
            .map_or(&[], |d| d.layers.as_slice())
    }

    fn layer(&self) -> Layer {
        self.layers()
            .get(self.layer_index)
            .map_or(Layer::Surface, |(layer, _)| *layer)
    }
}

fn preview_layer_button(width: f32) -> [f32; 4] {
    [8., 36., (width / 2. - 22.).clamp(1., 124.), 18.]
}

fn cache_layer_label(layer: Layer) -> String {
    match layer {
        Layer::Surface => tr(
            "Surface",
            "Поверхность",
            "Surface",
            "Superficie",
            "Oberfläche",
            "地表",
        )
        .into(),
        Layer::Below(y) => localized_format!(
            "Below Y {y}",
            "Ниже Y {y}",
            "Sous Y {y}",
            "Por debajo de Y {y}",
            "Unter Y {y}",
            "Y {y} 以下"
        ),
    }
}

fn over_preview_controls(mouse: [f32; 2], width: f32, height: f32) -> bool {
    inside(mouse, map_footer(width, height))
        || inside(mouse, preview_move_button(width))
        || inside(mouse, preview_layer_button(width))
        || (0..4).any(|i| inside(mouse, tab(width, i)))
}

fn open_preview(
    game: &Game<'_>,
    entry: &runtime::cache_manager::Entry,
) -> Result<CachePreview, String> {
    let directory = game.native_data_dir(runtime::MOD_ID).join("cache");
    let world = entry.world.clone().ok_or("Unknown cache identity")?;
    let file = entry.file.clone();
    let (wake, changes) = std::sync::mpsc::sync_channel(1);
    let (updates, response) = std::sync::mpsc::sync_channel(1);
    let request = Arc::new(Mutex::new(None::<PreviewRequest>));
    let prepared = Arc::new(Mutex::new(None));
    let (requested, output) = (request.clone(), prepared.clone());
    let preview_world = world.clone();
    std::thread::Builder::new()
        .name("map-preview".into())
        .spawn(move || {
            let mut preview = match maps::Preview::open(&directory, &file, &preview_world) {
                Ok(preview) => preview,
                Err(error) => {
                    let _ = updates.send(PreviewUpdate::Error(error.to_string()));
                    return;
                }
            };
            if updates
                .send(PreviewUpdate::Ready(
                    preview.dimensions.clone(),
                    preview.position,
                ))
                .is_err()
            {
                return;
            }
            let mut pending = false;
            let mut portals_pending = false;
            let mut previous = None::<(View, View, Arc<maps::TerrainAtlas>)>;
            loop {
                use std::sync::mpsc::RecvTimeoutError;
                let change = if pending || portals_pending {
                    changes.recv_timeout(Duration::from_millis(8))
                } else {
                    changes.recv().map_err(|_| RecvTimeoutError::Disconnected)
                };
                if change == Err(RecvTimeoutError::Disconnected) {
                    break;
                }
                let current = requested.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let Some(PreviewRequest {
                    view,
                    target,
                    point,
                }) = current
                else {
                    continue;
                };
                let atlas = if !pending
                    && let Some((old, old_target, atlas)) = &previous
                    && *old == view
                    && *old_target == target
                {
                    atlas.clone()
                } else {
                    match preview.prepare(&view, &target) {
                        Ok(atlas) => {
                            pending = preview.preparing();
                            previous = Some((view.clone(), target, atlas.clone()));
                            atlas
                        }
                        Err(error) => {
                            let _ = updates.send(PreviewUpdate::Error(error.to_string()));
                            break;
                        }
                    }
                };
                let column =
                    point.and_then(|point| preview.column(view.dimension, view.layer, point));
                let portals = preview.portals(&view);
                portals_pending = preview.portals_pending();
                *output.lock().unwrap_or_else(|e| e.into_inner()) = Some(PreparedPreview {
                    dimension: view.dimension,
                    layer: view.layer,
                    atlas,
                    point,
                    column,
                    portals,
                });
            }
        })
        .map_err(|error| error.to_string())?;
    let controller = runtime::initialize(game.native_data_dir(runtime::MOD_ID));
    let waypoints = controller.waypoints(&world);
    let waypoint_letters = controller.settings().waypoint_letters;
    let waypoint_labels = runtime::waypoint_labels(&waypoints, waypoint_letters);
    Ok(CachePreview {
        name: entry.name.clone(),
        world,
        menu: None,
        form: None,
        notice: None,
        waypoints,
        waypoint_labels,
        waypoint_letters,
        wake,
        request,
        prepared,
        response,
        dimensions: Vec::new(),
        layer_index: 0,
        position: None,
        camera: maps::Camera::new([0.; 2], 2.),
        last_frame: Instant::now(),
        dimension: Dimension::Overworld,
        uploaded: None,
        page_images: Vec::new(),
        dragging: false,
        error: None,
    })
}

fn draw_cache_frontend(game: &Game<'_>, batch: &mut HudBatch, state: &mut State) {
    if let Some(preview) = &mut state.caches.preview {
        draw_preview(game, batch, preview);
    } else {
        draw_cache_screen(game, batch, state);
    }
}

fn draw_preview(game: &Game<'_>, batch: &mut HudBatch, preview: &mut CachePreview) {
    use std::sync::mpsc::TryRecvError;
    match preview.response.try_recv() {
        Ok(PreviewUpdate::Ready(dimensions, position)) => {
            if let Some(first) = dimensions.first() {
                preview.dimension = first.dimension;
                if let Some(&(_, center)) = first.layers.first() {
                    preview.camera.snap(center);
                }
            }
            preview.position = position;
            preview.dimensions = dimensions;
            if preview.dimensions.is_empty() {
                preview.error = Some(
                    tr(
                        "No saved terrain",
                        "Нет сохранённого рельефа",
                        "Aucun terrain enregistré",
                        "No hay terreno guardado",
                        "Kein Gelände gespeichert",
                        "没有已保存的地形",
                    )
                    .into(),
                );
            }
        }
        Ok(PreviewUpdate::Error(error)) => preview.error = Some(runtime::localize_message(&error)),
        Err(TryRecvError::Disconnected) if preview.error.is_none() => {
            preview.error = Some(
                tr(
                    "Cache worker stopped",
                    "Поток кеша остановлен",
                    "Gestion du cache arrêtée",
                    "Gestor detenido",
                    "Cache-Verarbeitung beendet",
                    "缓存处理已停止",
                )
                .into(),
            );
        }
        _ => {}
    }
    game.native_fill(batch, [0., 0., batch.width, batch.height], 0xff181b20);
    let rect = rectangle(batch.width, batch.height);
    let now = Instant::now();
    preview
        .camera
        .advance((now - preview.last_frame).as_secs_f64());
    preview.last_frame = now;
    let view = large_camera_view(
        0,
        preview.dimension,
        preview.layer(),
        &preview.camera,
        rect,
        batch.scale,
    );
    let mouse = game.cursor();
    let point = preview.menu.map(|menu| menu.block).or_else(|| {
        (preview.form.is_none() && !over_preview_controls(mouse, batch.width, batch.height))
            .then(|| hovered_block(&view, rect, mouse))
            .flatten()
    });
    if preview.error.is_none() && !preview.dimensions.is_empty() {
        let request = PreviewRequest {
            target: camera_target(&view, &preview.camera),
            view: view.clone(),
            point,
        };
        let mut latest = preview.request.lock().unwrap_or_else(|e| e.into_inner());
        if latest.as_ref() != Some(&request) {
            *latest = Some(request);
            let _ = preview.wake.try_send(());
        }
    }
    let prepared = preview
        .prepared
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .filter(|prepared| prepared.dimension == view.dimension && prepared.layer == view.layer);
    let atlas = prepared.as_ref().map(|prepared| prepared.atlas.clone());
    if let Some(atlas) = &atlas
        && preview
            .uploaded
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(old, atlas))
    {
        sync_page_images(&mut preview.page_images, Some(atlas));
        preview.uploaded = Some(atlas.clone());
    }
    let settings = runtime::initialize(game.native_data_dir(runtime::MOD_ID)).settings();
    if preview.waypoint_letters != settings.waypoint_letters {
        preview.waypoint_letters = settings.waypoint_letters;
        preview.waypoint_labels =
            runtime::waypoint_labels(&preview.waypoints, preview.waypoint_letters);
    }
    let marker = preview
        .position
        .filter(|at| at.dimension == view.dimension)
        .map(|at| PlayerMarker {
            at: [f64::from(at.block[0]) + 0.5, f64::from(at.block[2]) + 0.5],
            yaw: at.yaw.map(f64::from),
            colors: (0xff343434, 0xffaaaaaa),
        });
    let frame = maps::Frame {
        view: view.clone(),
        atlas,
        raster_source: None,
        serial: 0,
        pixels: Vec::new(),
        known: 0,
    };
    draw_map_scene(
        game,
        batch,
        MapScene {
            rect,
            frame: &frame,
            images: &preview.page_images,
            hover: point,
            waypoints: &preview.waypoints,
            labels: &preview.waypoint_labels,
            marker,
            settings: &settings,
        },
        |batch| {
            if let Some(prepared) = &prepared {
                for &(at, kind) in prepared.portals.iter() {
                    draw_portal_marker(game, batch, &view, rect, at, kind);
                }
            }
        },
    );
    button(
        game,
        batch,
        ButtonIcon::Back,
        tab(batch.width, 0),
        tr("Back", "Назад", "Retour", "Atrás", "Zurück", "返回"),
        mouse,
        true,
    );
    for (index, dimension) in Dimension::ALL.into_iter().enumerate().skip(1) {
        let tab = tab(batch.width, index);
        let available = preview.dimensions.iter().any(|d| d.dimension == dimension);
        draw_dimension_tab(
            game,
            batch,
            tab,
            dimension,
            dimension == view.dimension,
            available,
        );
    }
    button(
        game,
        batch,
        ButtonIcon::Move,
        preview_move_button(batch.width),
        move_view_label(),
        mouse,
        !preview.dimensions.is_empty(),
    );
    if !preview.layers().is_empty() {
        button(
            game,
            batch,
            ButtonIcon::Next,
            preview_layer_button(batch.width),
            &cache_layer_label(preview.layer()),
            mouse,
            preview.layers().len() > 1,
        );
    }
    let footer = if let Some(error) = &preview.error {
        error.clone()
    } else if preview.dimensions.is_empty() {
        tr(
            "Loading map…",
            "Загрузка карты…",
            "Chargement de la carte…",
            "Cargando mapa…",
            "Karte wird geladen…",
            "正在加载地图…",
        )
        .into()
    } else if let Some(at) = preview.position {
        format!(
            "{} · {}: {} {} {} ({})",
            preview.name,
            tr(
                "Last position",
                "Последняя позиция",
                "Dernière position",
                "Última posición",
                "Letzte Position",
                "上次位置"
            ),
            at.block[0],
            at.block[1],
            at.block[2],
            at.dimension.label()
        )
    } else {
        preview.name.clone()
    };
    let panel = map_footer(batch.width, batch.height);
    game.native_fill(batch, panel, 0xdf10151d);
    let column = prepared
        .as_ref()
        .filter(|prepared| prepared.point == point)
        .and_then(|prepared| prepared.column.as_ref());
    if let Some([x, z]) = point {
        let height = column
            .and_then(|column| {
                column
                    .height
                    .map(|y| format!("  Y {}{y}", if column.approximate { "≈" } else { "" }))
            })
            .unwrap_or_default();
        let nether = nether_coordinates(&view, [x, z]).unwrap_or_default();
        let position = format!(
            "{}: X {x}{height}  Z {z}   {nether}",
            view.dimension.label()
        );
        if view.dimension != Dimension::End {
            dimension_icon(
                game,
                batch,
                Dimension::Nether,
                [panel[0] + 5., panel[1] + 2.],
            );
        }
        game.map_text(
            batch,
            &fit_text(game, &position, panel[2] - 34.),
            [panel[0] + 25., panel[1] + 5.],
            0xffffff,
            true,
        );
        let biome = column.and_then(|column| column.biome.as_deref());
        biome_icon(game, batch, biome, [panel[0] + 5., panel[1] + 18.]);
        if let Some(biome) = biome {
            let label = localized_biome(biome);
            game.map_text(
                batch,
                &fit_text(game, &label, panel[2] / 2. - 26.),
                [panel[0] + 25., panel[1] + 21.],
                0xffffff,
                true,
            );
        }
    }
    game.map_text(
        batch,
        &fit_text(game, &footer, panel[2] / 2. - 8.),
        [panel[0] + panel[2] / 2., panel[1] + 21.],
        0xbac4d0,
        true,
    );
    if let Some((notice, at)) = &preview.notice
        && at.elapsed() < Duration::from_secs(2)
    {
        draw_tooltip(game, batch, notice, [12., (batch.height - 80.).max(12.)]);
    }
    if let Some(menu) = preview.menu {
        let subtitle = preview
            .waypoints
            .iter()
            .find(|w| Some(waypoint_spot(w)) == menu.waypoint)
            .map(|w| w.name.as_str());
        draw_context_menu(game, batch, menu, subtitle, &preview_menu_items(menu));
    }
    match preview.form.as_ref() {
        Some(PreviewForm::Waypoint(form)) => draw_name_panel(game, batch, form),
        Some(PreviewForm::Coordinates { text, invalid }) => {
            draw_coordinate_panel(game, batch, text, *invalid)
        }
        None => {}
    }
}

fn preview_menu_items(menu: Menu) -> Vec<(&'static str, MenuAction)> {
    let mut items = if menu.waypoint.is_some() {
        waypoint_menu(false)
    } else {
        vec![(create_waypoint_label(), MenuAction::Waypoint)]
    };
    items.push((move_view_label(), MenuAction::MoveView));
    items
}

fn preview_move_button(width: f32) -> [f32; 4] {
    [(width - 154.).max(8.), 36., 146., 18.]
}

fn coordinate_buttons(rect: [f32; 4]) -> ([f32; 4], [f32; 4]) {
    (
        [
            rect[0] + 10.,
            rect[1] + rect[3] - 26.,
            (rect[2] - 29.) / 2.,
            18.,
        ],
        [
            rect[0] + (rect[2] + 9.) / 2.,
            rect[1] + rect[3] - 26.,
            (rect[2] - 29.) / 2.,
            18.,
        ],
    )
}

fn draw_coordinate_panel(game: &Game<'_>, batch: &mut HudBatch, text: &str, invalid: bool) {
    game.native_fill(batch, [0., 0., batch.width, batch.height], 0x70000000);
    let rect = name_form_rect(batch.width, batch.height);
    game.native_fill(batch, rect, 0xf01b2028);
    game.map_text(
        batch,
        move_view_label(),
        [rect[0] + 10., rect[1] + 8.],
        0xffffff,
        true,
    );
    let field = [rect[0] + 10., rect[1] + 30., rect[2] - 20., 18.];
    game.native_fill(batch, field, 0xff0d1117);
    game.map_text(
        batch,
        &format!("{text}_"),
        [field[0] + 4., field[1] + 4.],
        0xffffff,
        false,
    );
    let hint = if invalid {
        tr(
            "Enter valid X Z",
            "Введите допустимые X Z",
            "Saisissez des X Z valides",
            "Introduce X Z válidas",
            "Gültige X Z eingeben",
            "请输入有效的 X 和 Z 坐标",
        )
    } else {
        tr(
            "Enter X and Z, e.g. -120 64",
            "Введите X и Z, например -120 64",
            "Saisissez X et Z, p. ex. -120 64",
            "Introduce X y Z, p. ej. -120 64",
            "X und Z eingeben, z. B. -120 64",
            "输入 X 和 Z，例如 -120 64",
        )
    };
    game.map_text(
        batch,
        &fit_text(game, hint, rect[2] - 20.),
        [rect[0] + 10., rect[1] + 54.],
        if invalid { 0xff8888 } else { 0xbac4d0 },
        false,
    );
    let (apply, cancel) = coordinate_buttons(rect);
    button(
        game,
        batch,
        ButtonIcon::Move,
        apply,
        move_view_label(),
        game.cursor(),
        true,
    );
    button(
        game,
        batch,
        ButtonIcon::Cancel,
        cancel,
        tr("Cancel", "Отмена", "Annuler", "Cancelar", "Abbrechen", "取消"),
        game.cursor(),
        true,
    );
}

fn open_preview_coordinates(preview: &mut CachePreview, block: Option<[i32; 2]>) {
    preview.dragging = false;
    preview.menu = None;
    preview.form = Some(PreviewForm::Coordinates {
        text: block.map_or_else(String::new, |[x, z]| format!("{x} {z}")),
        invalid: false,
    });
    runtime::set_text_input(true);
}

fn finish_preview_form(game: &Game<'_>, preview: &mut CachePreview, save: bool) {
    let Some(form) = preview.form.take() else {
        return;
    };
    if save {
        match form {
            PreviewForm::Coordinates { text, .. } => {
                let Some([x, z]) = maps::map_coordinates(&text) else {
                    preview.form = Some(PreviewForm::Coordinates {
                        text,
                        invalid: true,
                    });
                    return;
                };
                preview
                    .camera
                    .snap([f64::from(x) + 0.5, f64::from(z) + 0.5]);
            }
            PreviewForm::Waypoint(form) => {
                let controller = runtime::initialize(game.native_data_dir(runtime::MOD_ID));
                let changed = if let Some(spot) = form.rename {
                    controller.update_waypoint(
                        &preview.world,
                        spot.dimension,
                        spot.x,
                        spot.z,
                        &form.text,
                        form.color,
                    )
                } else if let Some(at) = form.target {
                    let biome = preview
                        .prepared
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .as_ref()
                        .filter(|p| p.dimension == at.dimension && p.point == Some([at.x, at.z]))
                        .and_then(|p| p.column.as_ref()?.biome.as_deref().map(str::to_owned));
                    controller.add_waypoint(Waypoint {
                        world: preview.world.clone(),
                        dimension: at.dimension,
                        x: at.x,
                        y: at.y,
                        z: at.z,
                        name: form.text,
                        color: form.color,
                        icon: runtime::WaypointIcon::Letter,
                        biome,
                        show_3d: true,
                    })
                } else {
                    false
                };
                refresh_preview_waypoints(preview, controller);
                preview.notice = Some((
                    if changed {
                        tr(
                            "Waypoint saved",
                            "Вейпоинт сохранён",
                            "Repère enregistré",
                            "Marcador guardado",
                            "Wegpunkt gespeichert",
                            "已保存路径点",
                        )
                    } else {
                        tr(
                            "Waypoint unchanged",
                            "Вейпоинт не изменён",
                            "Repère inchangé",
                            "Marcador sin cambios",
                            "Wegpunkt unverändert",
                            "路径点未更改",
                        )
                    }
                    .into(),
                    Instant::now(),
                ));
            }
        }
    }
    runtime::set_text_input(false);
}

fn refresh_preview_waypoints(preview: &mut CachePreview, controller: &runtime::Controller) {
    preview.waypoints = controller.waypoints(&preview.world);
    preview.waypoint_labels =
        runtime::waypoint_labels(&preview.waypoints, preview.waypoint_letters);
}

fn preview_form_press(
    game: &Game<'_>,
    preview: &mut CachePreview,
    screen: &mod_api::ScreenView<'_>,
) {
    let rect = name_form_rect(screen.rect[2], screen.rect[3]);
    let (save, cancel) = match preview.form.as_mut() {
        Some(PreviewForm::Coordinates { .. }) => coordinate_buttons(rect),
        Some(PreviewForm::Waypoint(form)) => {
            for (index, &color) in runtime::WAYPOINT_COLORS.iter().enumerate() {
                if inside(screen.mouse, color_swatch_at(rect, index)) {
                    form.color = color;
                    return;
                }
            }
            name_form_buttons(rect)
        }
        None => return,
    };
    if inside(screen.mouse, save) {
        finish_preview_form(game, preview, true);
    } else if inside(screen.mouse, cancel) {
        finish_preview_form(game, preview, false);
    }
}

fn perform_preview_menu(
    game: &Game<'_>,
    preview: &mut CachePreview,
    menu: Menu,
    action: MenuAction,
) {
    preview.menu = None;
    let controller = runtime::initialize(game.native_data_dir(runtime::MOD_ID));
    match action {
        MenuAction::MoveView => open_preview_coordinates(preview, Some(menu.block)),
        MenuAction::Waypoint => {
            let y = preview
                .prepared
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .filter(|p| p.dimension == menu.dimension && p.point == Some(menu.block))
                .and_then(|p| p.column.as_ref()?.height)
                .or(menu.height)
                .or_else(|| {
                    preview
                        .position
                        .filter(|p| p.dimension == menu.dimension)
                        .map(|p| p.block[1])
                })
                .unwrap_or(64);
            let text = localized_format!(
                "Waypoint {}",
                "Вейпоинт {}",
                "Repère {}",
                "Marcador {}",
                "Wegpunkt {}",
                "路径点 {}",
                preview.waypoints.len() + 1
            );
            preview.form = Some(PreviewForm::Waypoint(NameForm {
                text,
                color: runtime::WAYPOINT_COLORS
                    [preview.waypoints.len() % runtime::WAYPOINT_COLORS.len()],
                target: Some(Target {
                    dimension: menu.dimension,
                    x: menu.block[0],
                    y,
                    z: menu.block[1],
                }),
                rename: None,
                background: Mode::Caches,
            }));
            runtime::set_text_input(true);
        }
        MenuAction::EditWaypoint => {
            if let Some(waypoint) = preview
                .waypoints
                .iter()
                .find(|w| Some(waypoint_spot(w)) == menu.waypoint)
            {
                preview.form = Some(PreviewForm::Waypoint(waypoint_form(waypoint, Mode::Caches)));
                runtime::set_text_input(true);
            }
        }
        MenuAction::DeleteWaypoint => {
            if let Some(spot) = menu.waypoint {
                controller.remove_waypoint(&preview.world, spot.dimension, spot.x, spot.z);
                refresh_preview_waypoints(preview, controller);
            }
        }
        _ => {}
    }
}

fn cache_frontend_press(
    game: &Game<'_>,
    state: &mut State,
    screen: &mod_api::ScreenView<'_>,
    button: i32,
    canceled: &mut bool,
) {
    let Some(preview) = &mut state.caches.preview else {
        cache_press(game, state, screen, button, canceled);
        return;
    };
    *canceled = true;
    if preview.form.is_some() {
        if button == 0 {
            preview_form_press(game, preview, screen);
        }
        return;
    }
    let [_, _, width, height] = screen.rect;
    if button == 1 {
        preview.dragging = false;
        preview.menu = None;
        if preview.dimensions.is_empty() || over_preview_controls(screen.mouse, width, height) {
            return;
        }
        let rect = rectangle(width, height);
        let view = large_camera_view(
            0,
            preview.dimension,
            preview.layer(),
            &preview.camera,
            rect,
            1.,
        );
        if let Some(waypoint) = waypoint_hit(
            game,
            &preview.waypoints,
            &preview.waypoint_labels,
            &view,
            rect,
            screen.mouse,
        ) {
            preview.menu = Some(waypoint_context(waypoint, screen.mouse));
        } else if let Some(block) = hovered_block(&view, rect, screen.mouse) {
            let prepared = preview.prepared.lock().unwrap_or_else(|e| e.into_inner());
            let column = prepared
                .as_ref()
                .filter(|p| p.dimension == view.dimension && p.point == Some(block))
                .and_then(|p| p.column.as_ref());
            preview.menu = Some(Menu {
                block,
                height: column.and_then(|column| column.height),
                height_approximate: column.is_some_and(|c| c.approximate),
                fluid: None,
                dimension: view.dimension,
                at: screen.mouse,
                portal: None,
                waypoint: None,
            });
        }
        return;
    }
    if button != 0 {
        return;
    }
    if let Some(menu) = preview.menu.take() {
        let items = preview_menu_items(menu);
        let subtitle = preview
            .waypoints
            .iter()
            .find(|w| Some(waypoint_spot(w)) == menu.waypoint)
            .map(|w| w.name.as_str());
        let (_, rows) = menu_layout(game, [width, height], menu, subtitle, &items);
        if let Some(((_, action), _)) = items
            .iter()
            .zip(rows)
            .find(|(_, row)| inside(screen.mouse, *row))
        {
            perform_preview_menu(game, preview, menu, *action);
        }
        return;
    }
    if inside(screen.mouse, preview_layer_button(width)) && preview.layers().len() > 1 {
        preview.layer_index = (preview.layer_index + 1) % preview.layers().len();
        preview.dragging = false;
        return;
    }
    if inside(screen.mouse, preview_move_button(width)) && !preview.dimensions.is_empty() {
        open_preview_coordinates(preview, None);
        return;
    }
    if inside(screen.mouse, tab(screen.rect[2], 0)) {
        state.caches.preview = None;
        return;
    }
    preview.dragging = false;
    for entry in &preview.dimensions {
        if inside(screen.mouse, tab(screen.rect[2], entry.dimension as usize)) {
            if preview.dimension != entry.dimension
                && let Some(&(_, center)) = entry.layers.first()
            {
                preview.dimension = entry.dimension;
                preview.layer_index = 0;
                preview.camera.snap(center);
            }
            return;
        }
    }
    preview.dragging = !over_preview_controls(screen.mouse, screen.rect[2], screen.rect[3]);
}

fn show_cache_manager(game: &Game<'_>, state: &mut State) {
    state.set_mode(Mode::Caches);
    state.menu = None;
    state.info = None;
    state.name_form = None;
    state.caches.scrollbar_grab = None;
    state.caches.preview = None;
    state.caches.confirm = None;
    state.caches.confirm_delete = None;
    runtime::set_text_input(false);
    refresh_cache_catalog(game);
}
pub(super) fn open_cache_manager(game: &mut Game<'_>) {
    if game.title.is_some() {
        if game.screen == super::Screen::Options {
            game.settings.save(&Game::options_path());
            game.set_screen(super::Screen::Playing);
            if game.screen != super::Screen::Playing {
                return;
            }
        }
        if game.screen != super::Screen::Playing {
            return;
        }
        if let Some(title) = game.title.as_mut() {
            title.mode = horizon_client::title::Mode::Main;
        }
        game.pad_clear_focus();
        let mut state = state();
        state.caches.title_open = true;
        show_cache_manager(game, &mut state);
        return;
    }
    if game.native_player().is_none() {
        return;
    }
    if game.screen == super::Screen::Options {
        game.settings.save(&Game::options_path());
        game.screen = super::Screen::Playing;
    }
    if game.mod_screen_open(runtime::MOD_ID) {
        show_cache_manager(game, &mut state());
    }
}
fn update_cache_ui(game: &Game<'_>, state: &mut State) {
    let snapshot = runtime::initialize(game.native_data_dir(runtime::MOD_ID))
        .caches
        .snapshot();
    if state.caches.catalog_revision != snapshot.revision {
        state.caches.catalog_revision = snapshot.revision;
        let file = runtime::container::world_path(std::path::Path::new(""), &state.world);
        state.caches.attention = snapshot.entries.iter().any(|e| {
            (e.world.as_ref() == Some(&state.world)
                || file.file_name().is_some_and(|f| f == e.file.as_str()))
                && e.compatibility != runtime::container::Compatibility::Current
        });
    }
    if state.mode != Mode::Caches {
        return;
    }
    if let Some(rx) = &state.caches.operation {
        match rx.try_recv() {
            Ok(result) => {
                state.caches.operation = None;
                state.caches.message = Some(match result {
                    Ok(()) if state.caches.deleting => tr(
                        "Cache deleted",
                        "Кеш удалён",
                        "Cache supprimé",
                        "Caché eliminada",
                        "Cache gelöscht",
                        "已删除缓存",
                    )
                    .into(),
                    Ok(()) => tr(
                        "Rebuild scheduled; images update when this cache is viewed",
                        "Перестроение назначено; изображения обновятся при просмотре кеша",
                        "Reconstruction planifiée à la prochaine consultation",
                        "Reconstrucción programada al consultar la caché",
                        "Neuaufbau beim Anzeigen geplant",
                        "已安排重建，查看此缓存时会更新图像",
                    )
                    .into(),
                    Err(error) => runtime::localize_message(&error),
                });
                state.caches.deleting = false;
                refresh_cache_catalog(game);
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                state.caches.operation = None;
                state.caches.message = Some(
                    tr(
                        "Cache worker stopped",
                        "Поток кеша остановлен",
                        "Gestion du cache arrêtée",
                        "Gestor detenido",
                        "Cache-Verarbeitung beendet",
                        "缓存处理已停止",
                    )
                    .into(),
                );
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }
    if state.caches.revision != snapshot.revision {
        state.caches.confirm_delete = None;
        state.caches.revision = snapshot.revision;
        state.caches.confirm = None;
        state.caches.icons.clear();
        if state
            .caches
            .selected
            .as_ref()
            .is_some_and(|name| !snapshot.entries.iter().any(|e| e.file == *name))
        {
            state.caches.selected = None;
        }
    }
    let height = game.gui_size().1;
    let start = state
        .caches
        .scroll
        .offset(snapshot.entries.len().saturating_sub(cache_rows(height)));
    let visible: BTreeSet<_> = snapshot
        .entries
        .iter()
        .skip(start)
        .take(cache_rows(height))
        .map(|e| e.file.as_str())
        .collect();
    state
        .caches
        .icons
        .retain(|file, _| visible.contains(file.as_str()));
    for entry in snapshot.entries.iter().skip(start).take(cache_rows(height)) {
        if let Some(pixels) = &entry.icon {
            state
                .caches
                .icons
                .entry(entry.file.clone())
                .or_insert_with(|| super::minimap_pages::create(64, 64, (**pixels).clone()));
        }
    }
}
fn draw_cache_screen(game: &Game<'_>, batch: &mut HudBatch, state: &State) {
    let snapshot = runtime::initialize(game.native_data_dir(runtime::MOD_ID))
        .caches
        .snapshot();
    game.native_fill(batch, [0., 0., batch.width, batch.height], 0xf0181b20);
    let title = cache_text(
        game,
        tr(
            "Map caches",
            "Кеши карт",
            "Caches des cartes",
            "Cachés de mapas",
            "Karten-Caches",
            "地图缓存",
        ),
        (batch.width - 166.).max(1.),
    );
    game.map_text(batch, &title, [12., 14.], 0xffffff, true);
    button(
        game,
        batch,
        ButtonIcon::Refresh,
        [batch.width - 92., 10., 80., 18.],
        tr(
            "Refresh",
            "Обновить",
            "Actualiser",
            "Actualizar",
            "Aktualisieren",
            "刷新",
        ),
        game.cursor(),
        !snapshot.loading && state.caches.operation.is_none(),
    );
    let start = state.caches.scroll.offset(
        snapshot
            .entries
            .len()
            .saturating_sub(cache_rows(batch.height)),
    );
    for forwards in [false, true] {
        button(
            game,
            batch,
            if forwards {
                ButtonIcon::Next
            } else {
                ButtonIcon::Previous
            },
            cache_page_button(batch.width, forwards),
            "",
            game.cursor(),
            if forwards {
                start + cache_rows(batch.height) < snapshot.entries.len()
            } else {
                start > 0
            },
        );
    }
    let track = cache_scrollbar(batch.width, batch.height);
    let bar = runtime::ListScrollbar::new(
        track[1],
        track[3],
        cache_rows(batch.height),
        snapshot.entries.len(),
        start,
    );
    game.native_fill(batch, track, 0xff232c35);
    game.native_fill(
        batch,
        [track[0], bar.thumb_top, track[2], bar.thumb_height],
        if bar.max_row == 0 {
            0xff46515c
        } else if state.caches.scrollbar_grab.is_some() || inside(game.cursor(), track) {
            0xffc0d6eb
        } else {
            0xff8b9bab
        },
    );
    for (i, entry) in snapshot
        .entries
        .iter()
        .skip(start)
        .take(cache_rows(batch.height))
        .enumerate()
    {
        let row = cache_row(batch.width, i);
        game.native_fill(
            batch,
            row,
            if state.caches.selected.as_ref() == Some(&entry.file) {
                0xff334658
            } else {
                0xff232c35
            },
        );
        if let Some(icon) = state.caches.icons.get(&entry.file) {
            let x = f64::from(row[0] + 6.);
            let y = f64::from(row[1] + 6.);
            super::minimap_pages::paint(
                batch,
                icon,
                &[
                    [x, y, 0., 0.],
                    [x + 32., y, 64., 0.],
                    [x + 32., y + 32., 64., 64.],
                    [x, y + 32., 0., 64.],
                ],
            );
        } else {
            game.native_fill(batch, [row[0] + 6., row[1] + 6., 32., 32.], 0xff506846);
            game.map_text(
                batch,
                if entry
                    .world
                    .as_ref()
                    .is_some_and(|w| w.starts_with("server:"))
                {
                    tr("S", "С", "S", "S", "S", "南")
                } else {
                    tr("W", "М", "M", "M", "W", "西")
                },
                [row[0] + 18., row[1] + 18.],
                0xffffff,
                true,
            );
        }
        let title = cache_text(game, &entry.name, row[2] - 52.);
        game.map_text(batch, &title, [row[0] + 44., row[1] + 4.], 0xffffff, false);
        let detail = cache_text(
            game,
            &localized_format!(
                "{:.2} MiB · {} · {}",
                "{:.2} МиБ · {} · {}",
                "{:.2} Mio · {} · {}",
                "{:.2} MiB · {} · {}",
                "{:.2} MiB · {} · {}",
                "{:.2} MiB · {} · {}",
                entry.bytes as f64 / 1048576.,
                cache_kind(entry.world.as_deref()),
                entry.file
            ),
            row[2] - 52.,
        );
        game.map_text(
            batch,
            &detail,
            [row[0] + 44., row[1] + 17.],
            0xbac4d0,
            false,
        );
        let label = cache_text(game, cache_status(&entry.compatibility), row[2] - 52.);
        game.map_text(
            batch,
            &label,
            [row[0] + 44., row[1] + 30.],
            if entry.compatibility == runtime::container::Compatibility::Current {
                0x9fc792
            } else {
                0xffd24a
            },
            false,
        );
    }
    if snapshot.entries.is_empty() {
        game.map_text(
            batch,
            tr(
                "No map cache files",
                "Файлов кеша карт нет",
                "Aucun cache",
                "No hay cachés",
                "Keine Karten-Caches",
                "没有地图缓存文件",
            ),
            [12., 46.],
            0xbac4d0,
            false,
        );
    }
    let busy = snapshot.loading || state.caches.operation.is_some();
    let selected = snapshot
        .entries
        .iter()
        .find(|e| Some(&e.file) == state.caches.selected.as_ref());
    if let Some(entry) = selected {
        let identity = cache_text(
            game,
            &format!(
                "{} · {}",
                cache_kind(entry.world.as_deref()),
                entry
                    .world
                    .as_deref()
                    .and_then(|w| w.split_once(':').map(|(_, id)| id))
                    .unwrap_or(&entry.file)
            ),
            batch.width - 24.,
        );
        game.map_text(batch, &identity, [12., batch.height - 73.], 0xbac4d0, false);
    }
    let can_rebuild = selected.is_some_and(|e| {
        e.world.is_some()
            && matches!(
                e.compatibility,
                runtime::container::Compatibility::Current
                    | runtime::container::Compatibility::RebuildImages
            )
    });
    button(
        game,
        batch,
        ButtonIcon::Refresh,
        cache_footer(batch.width, batch.height, 0),
        if state.caches.confirm.is_some() {
            tr(
                "Confirm rebuild",
                "Подтвердить",
                "Confirmer",
                "Confirmar",
                "Bestätigen",
                "确认重建",
            )
        } else {
            tr(
                "Rebuild images",
                "Перестроить",
                "Reconstruire",
                "Reconstruir",
                "Neu aufbauen",
                "重建图像",
            )
        },
        game.cursor(),
        can_rebuild && !busy,
    );
    for (index, label, enabled) in [
        (
            1,
            tr("View", "Просмотр", "Voir", "Ver", "Ansehen", "查看"),
            can_rebuild && !busy && selected.is_some_and(|entry| !is_active_cache(state, entry)),
        ),
        (
            2,
            if state.caches.confirm_delete.is_some() {
                tr(
                    "Confirm",
                    "Подтвердить",
                    "Confirmer",
                    "Confirmar",
                    "Bestätigen",
                    "确认",
                )
            } else {
                tr("Delete", "Удалить", "Supprimer", "Eliminar", "Löschen", "删除")
            },
            !busy
                && selected.is_some_and(|entry| {
                    entry.deletion.is_some() && !is_active_cache(state, entry)
                }),
        ),
        (3, tr("Back", "Назад", "Retour", "Atrás", "Zurück", "返回"), true),
    ] {
        let rect = cache_footer(batch.width, batch.height, index);
        let label = cache_text(game, label, rect[2] - 4.);
        button(
            game,
            batch,
            [
                ButtonIcon::Refresh,
                ButtonIcon::View,
                ButtonIcon::Delete,
                ButtonIcon::Back,
            ][index],
            rect,
            &label,
            game.cursor(),
            enabled,
        );
    }
    let diagnostic = snapshot.error.as_deref().map(runtime::localize_message);
    let message = state
        .caches
        .message
        .as_deref()
        .or(diagnostic.as_deref())
        .unwrap_or(if busy {
            tr(
                "Working…",
                "Обработка…",
                "En cours…",
                "Procesando…",
                "Verarbeitung…",
                "处理中…",
            )
        } else if state.caches.confirm_delete.is_some() {
            tr(
                "Delete the selected terrain cache permanently?",
                "Удалить выбранный кеш карты безвозвратно?",
                "Supprimer définitivement le cache sélectionné ?",
                "¿Eliminar definitivamente la caché seleccionada?",
                "Ausgewählten Karten-Cache endgültig löschen?",
                "永久删除选中的地形缓存？",
            )
        } else if state.caches.confirm.is_some() {
            tr(
                "Only images are rebuilt; saved terrain is retained",
                "Перестроятся только изображения; исходная карта сохранится",
                "Seules les images changent, le terrain est conservé",
                "Solo se reconstruyen imágenes; se conserva el terreno",
                "Nur Bilder werden neu aufgebaut; Gelände bleibt erhalten",
                "仅重建地图图像，保留已保存的地形",
            )
        } else {
            tr(
                "Wheel / ‹ › to browse. Select a cache to rebuild its images",
                "Колесо / ‹ › — листать. Выберите кеш для перестроения изображений",
                "Molette / ‹ › : parcourir. Sélectionnez un cache à reconstruire",
                "Rueda / ‹ › para navegar. Selecciona una caché para reconstruir imágenes",
                "Mausrad / ‹ › zum Blättern. Cache zum Neuaufbau der Bilder auswählen",
                "用滚轮或 ‹ › 翻页。选择缓存可重建图像",
            )
        });
    let message = cache_text(game, message, batch.width - 24.);
    game.map_text(batch, &message, [12., batch.height - 29.], 0xffd24a, false);
    let count = localized_format!(
        "Caches: {}–{} / {}{}",
        "Кеши: {}–{} / {}{}",
        "Caches : {}–{} / {}{}",
        "Cachés: {}–{} / {}{}",
        "Caches: {}–{} / {}{}",
        "缓存：{}–{} / {}{}",
        if snapshot.entries.is_empty() {
            0
        } else {
            start + 1
        },
        (start + cache_rows(batch.height)).min(snapshot.entries.len()),
        snapshot.entries.len(),
        if snapshot.truncated { "+" } else { "" }
    );
    game.map_text(batch, &count, [12., batch.height - 15.], 0xbac4d0, false);
}
fn cache_press(
    game: &Game<'_>,
    state: &mut State,
    screen: &mod_api::ScreenView<'_>,
    button_id: i32,
    canceled: &mut bool,
) {
    *canceled = true;
    if button_id != 0 {
        return;
    }
    let controller = runtime::initialize(game.native_data_dir(runtime::MOD_ID));
    let snapshot = controller.caches.snapshot();
    state.caches.scrollbar_grab = None;
    let max = snapshot
        .entries
        .len()
        .saturating_sub(cache_rows(screen.rect[3]));
    for forwards in [false, true] {
        if inside(screen.mouse, cache_page_button(screen.rect[2], forwards)) {
            state
                .caches
                .scroll
                .page(forwards, cache_rows(screen.rect[3]), max);
            return;
        }
    }
    if max > 0
        && inside(
            screen.mouse,
            cache_scrollbar(screen.rect[2], screen.rect[3]),
        )
    {
        let track = cache_scrollbar(screen.rect[2], screen.rect[3]);
        let bar = runtime::ListScrollbar::new(
            track[1],
            track[3],
            cache_rows(screen.rect[3]),
            snapshot.entries.len(),
            state.caches.scroll.offset(max),
        );
        state.caches.scrollbar_grab = Some(
            if screen.mouse[1] >= bar.thumb_top
                && screen.mouse[1] <= bar.thumb_top + bar.thumb_height
            {
                screen.mouse[1] - bar.thumb_top
            } else {
                bar.thumb_height / 2.
            },
        );
        cache_scroll_to(
            state,
            screen.rect[2],
            screen.rect[3],
            screen.mouse[1],
            snapshot.entries.len(),
        );
        return;
    }
    if inside(
        screen.mouse,
        cache_footer(screen.rect[2], screen.rect[3], 3),
    ) {
        close_cache_manager(state);
        return;
    }
    if snapshot.loading || state.caches.operation.is_some() {
        return;
    }
    if inside(screen.mouse, [screen.rect[2] - 92., 10., 80., 18.]) {
        state.caches.confirm = None;
        state.caches.confirm_delete = None;
        refresh_cache_catalog(game);
        return;
    }
    for index in [1, 2] {
        if !inside(
            screen.mouse,
            cache_footer(screen.rect[2], screen.rect[3], index),
        ) {
            continue;
        }
        state.caches.confirm = None;
        state.caches.message = None;
        let Some(entry) = snapshot
            .entries
            .iter()
            .find(|entry| Some(&entry.file) == state.caches.selected.as_ref())
            .filter(|entry| !is_active_cache(state, entry))
        else {
            return;
        };
        if index == 1 {
            state.caches.confirm_delete = None;
            if matches!(
                entry.compatibility,
                runtime::container::Compatibility::Current
                    | runtime::container::Compatibility::RebuildImages
            ) {
                match open_preview(game, entry) {
                    Ok(preview) => state.caches.preview = Some(preview),
                    Err(error) => state.caches.message = Some(runtime::localize_message(&error)),
                }
            }
        } else if let Some(selection) = entry.deletion.as_ref() {
            if state.caches.confirm_delete.as_ref() == Some(selection) {
                state.caches.confirm_delete = None;
                match controller
                    .maps
                    .delete_cache(selection.clone(), &state.world)
                {
                    Ok(rx) => {
                        state.caches.operation = Some(rx);
                        state.caches.deleting = true;
                    }
                    Err(error) => state.caches.message = Some(runtime::localize_message(&error)),
                }
            } else {
                state.caches.confirm_delete = Some(selection.clone());
            }
        }
        return;
    }
    if inside(
        screen.mouse,
        cache_footer(screen.rect[2], screen.rect[3], 0),
    ) {
        state.caches.confirm_delete = None;
        if let Some(entry) = snapshot
            .entries
            .iter()
            .find(|e| Some(&e.file) == state.caches.selected.as_ref())
            && matches!(
                entry.compatibility,
                runtime::container::Compatibility::Current
                    | runtime::container::Compatibility::RebuildImages
            )
            && let Some(world) = entry.world.as_deref()
        {
            if state.caches.confirm.as_deref() == Some(&entry.file) {
                state.caches.confirm = None;
                match controller.maps.rebuild_images(world, &entry.file) {
                    Ok(rx) => {
                        state.caches.operation = Some(rx);
                        state.caches.message = None;
                    }
                    Err(error) => state.caches.message = Some(runtime::localize_message(&error)),
                }
            } else {
                state.caches.confirm = Some(entry.file.clone());
                state.caches.message = None;
            }
        }
        return;
    }
    let start = state.caches.scroll.offset(
        snapshot
            .entries
            .len()
            .saturating_sub(cache_rows(screen.rect[3])),
    );
    for (i, entry) in snapshot
        .entries
        .iter()
        .skip(start)
        .take(cache_rows(screen.rect[3]))
        .enumerate()
    {
        if inside(screen.mouse, cache_row(screen.rect[2], i)) {
            state.caches.selected = Some(entry.file.clone());
            state.caches.confirm_delete = None;
            state.caches.confirm = None;
            state.caches.message = None;
            break;
        }
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

pub(super) fn covers_world(game: &Game<'_>) -> bool {
    game.map_on(runtime::MOD_ID)
        && game.mod_screen() == Some(runtime::MOD_ID)
        && game.native_player().is_some()
        && map_covers_world(&state())
}

pub(super) fn screen(game: &Game<'_>, batch: &mut HudBatch, screen: &mod_api::ScreenView<'_>) {
    apply_sdk_lifecycle(game);
    if !game.map_on(runtime::MOD_ID) {
        return;
    }
    if game.title.is_some() {
        runtime::set_language(&horizon_client::lang::code());
        let mut state = state();
        if screen.id == "title" {
            if state.caches.title_open {
                update_cache_ui(game, &mut state);
                draw_cache_frontend(game, batch, &mut state);
            } else {
                button(
                    game,
                    batch,
                    ButtonIcon::Cache,
                    title_cache_button(batch.width),
                    cache_title(),
                    game.cursor(),
                    true,
                );
            }
        } else if state.caches.title_open {
            close_cache_manager(&mut state);
        }
        return;
    }
    if game.mod_screen() != Some(runtime::MOD_ID) {
        return;
    }
    let mut state = state();
    match state.mode {
        Mode::Map => draw_map_screen(game, batch, &state),
        Mode::Waypoints => draw_waypoints_screen(game, batch, &state),
        Mode::Name => {
            if let Some(form) = state.name_form.as_ref() {
                draw_name_form(game, batch, &state, form);
            }
        }
        Mode::Caches => draw_cache_frontend(game, batch, &mut state),
    }
}

pub(super) fn press(
    game: &Game<'_>,
    screen: &mod_api::ScreenView<'_>,
    button: i32,
    canceled: &mut bool,
) {
    if game.title.is_some() && screen.id == "title" && game.map_on(runtime::MOD_ID) {
        let mut state = state();
        if state.caches.title_open {
            cache_frontend_press(game, &mut state, screen, button, canceled);
        } else if button == 0 && inside(screen.mouse, title_cache_button(screen.rect[2])) {
            state.caches.title_open = true;
            show_cache_manager(game, &mut state);
            *canceled = true;
        }
        return;
    }
    if game.mod_screen() != Some(runtime::MOD_ID) || !game.map_on(runtime::MOD_ID) {
        return;
    }
    let chosen = game.map_key(
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
        Mode::Caches => cache_frontend_press(game, &mut state, screen, button, canceled),
    }
}

fn map_press(
    game: &Game<'_>,
    state: &mut State,
    screen: &mod_api::ScreenView<'_>,
    button: i32,
    canceled: &mut bool,
) {
    if inside(screen.mouse, cache_map_button(screen.rect[2])) {
        if button == 0 {
            show_cache_manager(game, state);
        }
        *canceled = true;
        return;
    }
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
        let show_3d = menu
            .waypoint
            .map_or(true, |spot| waypoint_show_3d(state, spot));
        let items = menu_items(game, menu, target, show_3d);
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

fn waypoint_context(waypoint: &Waypoint, at: [f32; 2]) -> Menu {
    Menu {
        height_approximate: false,
        block: [waypoint.x, waypoint.z],
        height: Some(waypoint.y),
        fluid: None,
        dimension: waypoint.dimension,
        at,
        portal: None,
        waypoint: Some(waypoint_spot(waypoint)),
    }
}

fn menu_target_at(
    game: &Game<'_>,
    state: &State,
    screen: &mod_api::ScreenView<'_>,
    at: [f32; 2],
) -> Option<Menu> {
    let rect = rectangle(screen.rect[2], screen.rect[3]);
    let frame = state.frame.as_ref().filter(|frame| {
        frame.view.dimension == state.selected
            && frame.view.large
            && frame.view.session == state.session
    })?;
    if let Some(waypoint) = waypoint_at_pointer(game, state, &frame.view, rect, at) {
        return Some(waypoint_context(waypoint, at));
    }
    let block = hovered_block(&frame.view, rect, at)?;
    let portal = portal_near(state, state.selected, block);
    let column = if state.dimension == frame.view.dimension {
        hovered_column(game, frame.view.layer, block)
    } else {
        match frame.view.layer {
            Layer::Below(y) => Some(ColumnInfo {
                height: y,
                fluid: None,
            }),
            Layer::Surface => None,
        }
    };
    let height = if let Some((_, at)) = portal {
        Some(at[1])
    } else {
        column.map(|column| column.height)
    };
    let fluid = if portal.is_some() {
        None
    } else {
        column.and_then(|column| column.fluid)
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
        fluid,
        dimension: state.selected,
        at,
        portal: portal.map(|(kind, _)| kind),
        waypoint: None,
    })
}

fn waypoint_press(
    game: &Game<'_>,
    state: &mut State,
    screen: &mod_api::ScreenView<'_>,
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
            color: default_waypoint_color(state),
            target: Some(Target {
                dimension: Dimension::from_id(player.dimension),
                x: player.x.floor() as i32,
                y: player.y.floor() as i32,
                z: player.z.floor() as i32,
            }),
            rename: None,
            background: Mode::Waypoints,
        });
        state.set_mode(Mode::Name);
        runtime::set_text_input(true);
        *canceled = true;
        return;
    }
    if inside(screen.mouse, back) {
        state.set_mode(Mode::Map);
        *canceled = true;
        return;
    }
    for index in 0..state.waypoints.len() {
        if let Some(row) = waypoint_row_at(state, screen.rect[2], screen.rect[3], index)
            && inside(screen.mouse, waypoint_3d_rect(row))
            && let Some(waypoint) = state.waypoints.get(index)
        {
            runtime::initialize(game.native_data_dir(runtime::MOD_ID)).set_waypoint_3d(
                &state.world,
                waypoint.dimension,
                waypoint.x,
                waypoint.z,
                !waypoint.show_3d,
            );
            *canceled = true;
            return;
        }
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
            color: waypoint.color,
            target: None,
            rename: Some(Spot {
                dimension: waypoint.dimension,
                x: waypoint.x,
                z: waypoint.z,
            }),
            background: Mode::Waypoints,
        });
        state.set_mode(Mode::Name);
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
                "已删除路径点",
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
    screen: &mod_api::ScreenView<'_>,
    button: i32,
    canceled: &mut bool,
) {
    if button != 0 {
        return;
    }
    let rect = name_form_rect(screen.rect[2], screen.rect[3]);
    for (index, &color) in runtime::WAYPOINT_COLORS.iter().enumerate() {
        if inside(screen.mouse, color_swatch_at(rect, index)) {
            if let Some(form) = state.name_form.as_mut() {
                form.color = color;
            }
            *canceled = true;
            return;
        }
    }
    let (save, cancel) = name_form_buttons(rect);
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
        controller.update_waypoint(
            &state.world,
            rename.dimension,
            rename.x,
            rename.z,
            &form.text,
            form.color,
        );
        state.notice = Some((
            tr(
                "Waypoint updated",
                "Вейпоинт обновлён",
                "Repère modifié",
                "Marcador actualizado",
                "Wegpunkt aktualisiert",
                "已更新路径点",
            )
            .into(),
            Instant::now(),
        ));
    } else if let Some(target) = form.target {
        let biome = known_biome(game, target.dimension, [target.x, target.y, target.z]);
        let saved = controller.add_waypoint(Waypoint {
            world: state.world.clone(),
            dimension: target.dimension,
            x: target.x,
            y: target.y,
            z: target.z,
            name: Waypoint::clean_name(&form.text),
            color: form.color,
            icon: runtime::WaypointIcon::Letter,
            biome,
            show_3d: true,
        });
        state.notice = Some((
            if saved {
                tr(
                    "Waypoint saved",
                    "Вейпоинт сохранён",
                    "Repère enregistré",
                    "Marcador guardado",
                    "Wegpunkt gespeichert",
                    "已保存路径点",
                )
            } else {
                tr(
                    "Waypoint unchanged: duplicate, limit reached or world unavailable",
                    "Вейпоинт не изменён: дубликат, лимит или мир недоступен",
                    "Repère inchangé : doublon, limite atteinte ou monde indisponible",
                    "Marcador sin cambios: duplicado, límite alcanzado o mundo no disponible",
                    "Wegpunkt unverändert: Duplikat, Grenze erreicht oder Welt nicht verfügbar",
                    "路径点未更改，可能已存在、已达上限或世界不可用",
                )
            }
            .into(),
            Instant::now(),
        ));
    }
    state.set_mode(form.background);
    runtime::set_text_input(false);
}

fn cancel_name(state: &mut State) {
    if let Some(form) = state.name_form.take() {
        state.set_mode(form.background);
    }
    runtime::set_text_input(false);
}

pub(super) fn drag(game: &Game<'_>, delta: [f32; 2], button: i32, canceled: &mut bool) {
    if (game.mod_screen() != Some(runtime::MOD_ID)
        && !(game.title.is_some() && state().caches.title_open))
        || !game.map_on(runtime::MOD_ID)
        || button != 0
    {
        return;
    }
    let mut state = state();
    if state.mode == Mode::Caches {
        if let Some(preview) = &mut state.caches.preview {
            if preview.dragging {
                preview.camera.pan(delta.map(f64::from));
            }
            *canceled = true;
            return;
        }
        if state.caches.scrollbar_grab.is_some() {
            let snapshot = runtime::initialize(game.native_data_dir(runtime::MOD_ID))
                .caches
                .snapshot();
            let (width, height) = game.gui_size();
            cache_scroll_to(
                &mut state,
                width,
                height,
                game.cursor()[1],
                snapshot.entries.len(),
            );
        }
        *canceled = true;
        return;
    }
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
    screen: &mod_api::ScreenView<'_>,
    scroll: [f64; 2],
    canceled: &mut bool,
) {
    if (game.mod_screen() != Some(runtime::MOD_ID)
        && !(game.title.is_some() && state().caches.title_open))
        || !game.map_on(runtime::MOD_ID)
    {
        return;
    }
    let mut state = state();
    if state.mode == Mode::Caches {
        if let Some(preview) = &mut state.caches.preview {
            if preview.form.is_some() || preview.menu.is_some() {
                *canceled = true;
                return;
            }
            let rect = rectangle(screen.rect[2], screen.rect[3]);
            if !over_preview_controls(screen.mouse, screen.rect[2], screen.rect[3]) {
                preview.camera.zoom_at(
                    scroll[1],
                    [
                        f64::from(screen.mouse[0] - rect[0] - rect[2] / 2.),
                        f64::from(screen.mouse[1] - rect[1] - rect[3] / 2.),
                    ],
                );
            }
            *canceled = true;
            return;
        }
        let snapshot = runtime::initialize(game.native_data_dir(runtime::MOD_ID))
            .caches
            .snapshot();
        let max = snapshot
            .entries
            .len()
            .saturating_sub(cache_rows(screen.rect[3]));
        state.caches.scroll.scroll(scroll[1], max);
        *canceled = true;
        return;
    }
    if state.info.is_some() {
        let rect = information_rect(screen.rect[2], screen.rect[3]);
        let visible = ((rect[3] - 56.0) / 12.0).max(1.0) as usize;
        let lines = information_lines(game, &state, state.info.unwrap(), rect[2] - 24.0);
        let max = lines.len().saturating_sub(visible);
        state.info_scroll.scroll(scroll[1], max);
        *canceled = true;
        return;
    }
    if state.mode == Mode::Waypoints {
        let rows = state
            .waypoints
            .len()
            .saturating_sub(waypoint_visible_rows(screen.rect[3]));
        state.waypoint_scroll.scroll(scroll[1], rows);
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

fn title_cache_screen(game: &Game<'_>) -> bool {
    game.screen == super::Screen::Playing
        && game
            .title
            .as_ref()
            .is_some_and(|title| title.mode.id() == "title")
        && !game.prompt_open()
        && game.map_on(runtime::MOD_ID)
}

pub(super) fn title_cache_open(game: &Game<'_>) -> bool {
    title_cache_screen(game) && state().caches.title_open
}

pub(super) fn cache_pad_event(game: &Game<'_>, event: &winit::event::WindowEvent) -> bool {
    use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
    if !title_cache_screen(game) {
        return false;
    }
    let open = state().caches.title_open;
    let Some(screen) = game.screen_view_now() else {
        return false;
    };
    match event {
        WindowEvent::MouseInput {
            state: pressed,
            button,
            ..
        } => {
            let button = match button {
                MouseButton::Left => 0,
                MouseButton::Right => 1,
                _ => 2,
            };
            let mask = 1 << button;
            if *pressed == ElementState::Released {
                let mut state = state();
                let captured = state.caches.pad_buttons & mask != 0;
                state.caches.pad_buttons &= !mask;
                return captured || open;
            }
            let launch = button == 0 && inside(screen.mouse, title_cache_button(screen.rect[2]));
            if !open && !launch {
                return false;
            }
            state().caches.pad_buttons |= mask;
            let mut consumed = false;
            press(game, &screen, button, &mut consumed);
            true
        }
        WindowEvent::MouseWheel { delta, .. } if open => {
            let amount = match delta {
                MouseScrollDelta::LineDelta(x, y) => [f64::from(*x), f64::from(*y)],
                MouseScrollDelta::PixelDelta(p) => [p.x / 16., p.y / 16.],
            };
            let mut consumed = false;
            scroll(game, &screen, amount, &mut consumed);
            true
        }
        _ => false,
    }
}

pub(super) fn cache_pad_targets(game: &Game<'_>) -> Option<Vec<horizon_client::widgets::Target>> {
    use horizon_client::widgets::{Kind, Target};
    if !title_cache_screen(game) {
        return None;
    }
    let state = state();
    let (width, height) = game.gui_size();
    if !state.caches.title_open {
        let mut targets = super::key_focus::drawn_targets();
        targets.push(Target {
            rect: title_cache_button(width),
            kind: Kind::Button,
        });
        return Some(targets);
    }
    let mut rects = Vec::new();
    if let Some(preview) = &state.caches.preview {
        if let Some(form) = &preview.form {
            let panel = name_form_rect(width, height);
            let (save, cancel) = match form {
                PreviewForm::Waypoint(_) => {
                    rects.extend(
                        (0..runtime::WAYPOINT_COLORS.len()).map(|i| color_swatch_at(panel, i)),
                    );
                    name_form_buttons(panel)
                }
                PreviewForm::Coordinates { .. } => coordinate_buttons(panel),
            };
            rects.extend([save, cancel]);
        } else if let Some(menu) = preview.menu {
            let items = preview_menu_items(menu);
            let subtitle = preview
                .waypoints
                .iter()
                .find(|w| Some(waypoint_spot(w)) == menu.waypoint)
                .map(|w| w.name.as_str());
            rects = menu_layout(game, [width, height], menu, subtitle, &items).1;
        } else {
            rects.push(tab(width, 0));
            rects.extend(
                preview
                    .dimensions
                    .iter()
                    .map(|entry| tab(width, entry.dimension as usize)),
            );
            if preview.layers().len() > 1 {
                rects.push(preview_layer_button(width));
            }
            if !preview.dimensions.is_empty() {
                rects.push(preview_move_button(width));
            }
        }
    } else {
        let catalog = runtime::initialize(game.native_data_dir(runtime::MOD_ID))
            .caches
            .snapshot();
        let rows = cache_rows(height);
        let start = state
            .caches
            .scroll
            .offset(catalog.entries.len().saturating_sub(rows));
        rects.extend(
            (0..rows.min(catalog.entries.len().saturating_sub(start))).map(|i| cache_row(width, i)),
        );
        if start > 0 {
            rects.push(cache_page_button(width, false));
        }
        if start + rows < catalog.entries.len() {
            rects.push(cache_page_button(width, true));
        }
        if !catalog.loading && state.caches.operation.is_none() {
            rects.push([width - 92., 10., 80., 18.]);
            if let Some(entry) = catalog
                .entries
                .iter()
                .find(|entry| Some(&entry.file) == state.caches.selected.as_ref())
            {
                let can_read = entry.world.is_some()
                    && matches!(
                        entry.compatibility,
                        runtime::container::Compatibility::Current
                            | runtime::container::Compatibility::RebuildImages
                    );
                if can_read {
                    rects.extend([
                        cache_footer(width, height, 0),
                        cache_footer(width, height, 1),
                    ]);
                }
                if entry.deletion.is_some() {
                    rects.push(cache_footer(width, height, 2));
                }
            }
        }
        rects.push(cache_footer(width, height, 3));
    }
    Some(
        rects
            .into_iter()
            .map(|rect| Target {
                rect,
                kind: Kind::Button,
            })
            .collect(),
    )
}

pub(super) fn cache_pad_back(game: &Game<'_>) -> bool {
    if !title_cache_screen(game) || !state().caches.title_open {
        return false;
    }
    let mut canceled = false;
    screen_key(game, "key.keyboard.escape", &mut canceled);
    canceled
}

pub(super) fn screen_key(game: &Game<'_>, key: &str, canceled: &mut bool) {
    if (game.mod_screen() != Some(runtime::MOD_ID)
        && !(game.title.is_some() && state().caches.title_open))
        || !game.map_on(runtime::MOD_ID)
    {
        return;
    }
    let mut state = state();
    if state.mode == Mode::Caches {
        if let Some(preview) = state.caches.preview.as_mut() {
            if preview.form.is_some() {
                match key {
                    "key.keyboard.escape" => finish_preview_form(game, preview, false),
                    "key.keyboard.enter" | "key.keyboard.keypad.enter" => {
                        finish_preview_form(game, preview, true)
                    }
                    "key.keyboard.backspace" => match preview.form.as_mut() {
                        Some(PreviewForm::Waypoint(form)) => {
                            form.text.pop();
                        }
                        Some(PreviewForm::Coordinates { text, invalid }) => {
                            text.pop();
                            *invalid = false;
                        }
                        None => {}
                    },
                    _ => {}
                }
            } else if key == "key.keyboard.escape" && preview.menu.take().is_none() {
                state.caches.preview = None;
            }
            *canceled = true;
            return;
        }
        let snapshot = runtime::initialize(game.native_data_dir(runtime::MOD_ID))
            .caches
            .snapshot();
        let rows = cache_rows(game.gui_size().1);
        let max = snapshot.entries.len().saturating_sub(rows);
        match key {
            "key.keyboard.up" => state.caches.scroll.page(false, 1, max),
            "key.keyboard.down" => state.caches.scroll.page(true, 1, max),
            "key.keyboard.page.up" => state.caches.scroll.page(false, rows, max),
            "key.keyboard.page.down" => state.caches.scroll.page(true, rows, max),
            "key.keyboard.home" => state.caches.scroll.jump(0, max),
            "key.keyboard.end" => state.caches.scroll.jump(max, max),
            "key.keyboard.escape" => {
                if state.caches.confirm_delete.take().is_none()
                    && state.caches.confirm.take().is_none()
                {
                    close_cache_manager(&mut state);
                }
            }
            _ => {}
        }
        *canceled = true;
        return;
    }
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
    if (game.mod_screen() != Some(runtime::MOD_ID)
        && !(game.title.is_some() && state().caches.title_open))
        || !game.map_on(runtime::MOD_ID)
    {
        return;
    }
    let mut state = state();
    if let Some(form) = state.caches.preview.as_mut().and_then(|p| p.form.as_mut()) {
        match form {
            PreviewForm::Waypoint(form) if !c.is_control() && form.text.chars().count() < 32 => {
                form.text.push(c)
            }
            PreviewForm::Coordinates { text, invalid }
                if text.len() < 32
                    && (c.is_ascii_digit() || matches!(c, ' ' | ',' | '-' | '+')) =>
            {
                text.push(c);
                *invalid = false;
            }
            _ => {}
        }
        *canceled = true;
        return;
    }
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
    runtime::set_waypoint_background_transparent(false);
    runtime::initialize(game.native_data_dir(runtime::MOD_ID))
        .maps
        .reset(next);
}

pub(super) fn atlas_invalidated() {
    state().frame = None;
    state().page_images.clear();
}
