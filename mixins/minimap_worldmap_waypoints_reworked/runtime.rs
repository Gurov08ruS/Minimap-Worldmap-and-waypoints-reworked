// SPDX-License-Identifier: GPL-3.0-only
// Based on horizon-rust/horizon-native-mod-template (MIT); see LICENSE-MIT and NOTICE.
// Shared by Cargo tests and the injected game module. Keep modules inline.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU8, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};

pub const MOD_ID: &str = "minimap_worldmap_waypoints_reworked";
pub const MOD_TITLE: &str = "Minimap, Worldmap and waypoints reworked";
pub const WORLD_MAP_KEY: &str = "world_map";
pub const WORLD_MAP_DEFAULT: &str = "key.keyboard.m";
pub const WAYPOINTS_KEY: &str = "waypoints";
pub const WAYPOINTS_DEFAULT: &str = "key.keyboard.n";
pub const SETTINGS_FILE: &str = "settings.ini";
const SIZES: [u16; 7] = [64, 96, 128, 160, 192, 224, 256];
const ZOOMS: [f32; 7] = [0.1, 0.25, 0.5, 0.75, 1.0, 2.0, 4.0];

// Updated by the engine adapter; English is the standalone/default language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Language {
    English,
    Russian,
    French,
    Spanish,
    German,
}

impl Language {
    pub fn from_code(code: &str) -> Self {
        let prefix = code.split(['_', '-']).next().unwrap_or_default();
        if prefix.eq_ignore_ascii_case("ru") {
            Self::Russian
        } else if prefix.eq_ignore_ascii_case("fr") {
            Self::French
        } else if prefix.eq_ignore_ascii_case("es") {
            Self::Spanish
        } else if prefix.eq_ignore_ascii_case("de") {
            Self::German
        } else {
            Self::English
        }
    }
}

static LANGUAGE: AtomicU8 = AtomicU8::new(Language::English as u8);

pub fn set_language(code: &str) {
    LANGUAGE.store(Language::from_code(code) as u8, Ordering::Relaxed);
}

pub fn language() -> Language {
    match LANGUAGE.load(Ordering::Relaxed) {
        1 => Language::Russian,
        2 => Language::French,
        3 => Language::Spanish,
        4 => Language::German,
        _ => Language::English,
    }
}

pub fn tr(
    english: &'static str,
    russian: &'static str,
    french: &'static str,
    spanish: &'static str,
    german: &'static str,
) -> &'static str {
    match language() {
        Language::English => english,
        Language::Russian => russian,
        Language::French => french,
        Language::Spanish => spanish,
        Language::German => german,
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dimension {
    Player,
    Overworld,
    Nether,
    End,
}
impl Dimension {
    pub const ALL: [Self; 4] = [Self::Player, Self::Overworld, Self::Nether, Self::End];
    pub fn name(self) -> &'static str {
        match self {
            Self::Player => "player",
            Self::Overworld => "overworld",
            Self::Nether => "nether",
            Self::End => "end",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Player => tr("Player", "Игрок", "Joueur", "Jugador", "Spieler"),
            Self::Overworld => tr(
                "Overworld",
                "Верхний мир",
                "Surface",
                "Mundo normal",
                "Oberwelt",
            ),
            Self::Nether => tr("Nether", "Незер", "Nether", "Nether", "Nether"),
            Self::End => tr("End", "Энд", "End", "End", "Ende"),
        }
    }
    pub fn from_id(id: &str) -> Self {
        match id {
            "minecraft:the_nether" => Self::Nether,
            "minecraft:the_end" => Self::End,
            _ => Self::Overworld,
        }
    }
    /// The level id a command uses. Resolve `Player` before asking.
    pub fn id(self) -> &'static str {
        match self {
            Self::Nether => "minecraft:the_nether",
            Self::End => "minecraft:the_end",
            Self::Player | Self::Overworld => "minecraft:overworld",
        }
    }
    pub fn resolve(self, player: Self) -> Self {
        if self == Self::Player { player } else { self }
    }
    /// The same position as another dimension shows it. The Nether's
    /// horizontal coordinates are one eighth of the Overworld's; the End and
    /// the Overworld share one scale. `Player` is not a place: resolve it
    /// before converting.
    pub fn convert(self, to: Self, position: [f64; 2]) -> [f64; 2] {
        if self == Self::Player || to == Self::Player {
            return position;
        }
        let nether = |dimension: Self| dimension == Self::Nether;
        match (nether(self), nether(to)) {
            (false, true) => [position[0] / 8.0, position[1] / 8.0],
            (true, false) => [position[0] * 8.0, position[1] * 8.0],
            _ => position,
        }
    }
}

pub mod minimap {
    use super::{Language, language, tr};
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Corner {
        TopLeft,
        TopRight,
        BottomLeft,
        BottomRight,
    }

    impl Corner {
        pub const ALL: [Self; 4] = [
            Self::TopLeft,
            Self::TopRight,
            Self::BottomLeft,
            Self::BottomRight,
        ];

        pub fn name(self) -> &'static str {
            match self {
                Self::TopLeft => "top_left",
                Self::TopRight => "top_right",
                Self::BottomLeft => "bottom_left",
                Self::BottomRight => "bottom_right",
            }
        }

        fn label(self) -> &'static str {
            match self {
                Self::TopLeft => tr(
                    "Top left",
                    "Слева сверху",
                    "En haut à gauche",
                    "Arriba a la izquierda",
                    "Oben links",
                ),
                Self::TopRight => tr(
                    "Top right",
                    "Справа сверху",
                    "En haut à droite",
                    "Arriba a la derecha",
                    "Oben rechts",
                ),
                Self::BottomLeft => tr(
                    "Bottom left",
                    "Слева снизу",
                    "En bas à gauche",
                    "Abajo a la izquierda",
                    "Unten links",
                ),
                Self::BottomRight => tr(
                    "Bottom right",
                    "Справа снизу",
                    "En bas à droite",
                    "Abajo a la derecha",
                    "Unten rechts",
                ),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Settings {
        pub enabled: bool,
        pub corner: Corner,
        pub size_px: u16,
        pub blocks_per_pixel: f32,
        pub rotate_with_player: bool,
        pub round: bool,
        pub mini_dimension: super::Dimension,
        pub world_dimension: super::Dimension,
        pub smooth_worldmap: bool,
        pub overworld_coordinates: bool,
        pub nether_coordinates: bool,
    }

    impl Default for Settings {
        fn default() -> Self {
            Self {
                enabled: true,
                corner: Corner::TopRight,
                size_px: 128,
                blocks_per_pixel: 1.0,
                rotate_with_player: false,
                round: false,
                mini_dimension: super::Dimension::Player,
                world_dimension: super::Dimension::Player,
                smooth_worldmap: true,
                overworld_coordinates: true,
                nether_coordinates: true,
            }
        }
    }

    impl Settings {
        pub fn decode(text: &str) -> Self {
            let mut value = Self::default();
            for line in text.lines() {
                let Some((key, raw)) = line.split_once('=') else {
                    continue;
                };
                let raw = raw.trim();
                match key.trim() {
                    "mini_dimension" | "world_dimension" => {
                        if let Some(dimension) =
                            super::Dimension::ALL.into_iter().find(|d| d.name() == raw)
                        {
                            if key.trim() == "mini_dimension" {
                                value.mini_dimension = dimension;
                            } else {
                                value.world_dimension = dimension;
                            }
                        }
                    }
                    "overworld_coordinates" => {
                        value.overworld_coordinates =
                            raw.parse().unwrap_or(value.overworld_coordinates)
                    }
                    "nether_coordinates" => {
                        value.nether_coordinates = raw.parse().unwrap_or(value.nether_coordinates)
                    }
                    "smooth_worldmap" => {
                        value.smooth_worldmap = raw.parse().unwrap_or(value.smooth_worldmap)
                    }
                    "enabled" => value.enabled = raw.parse().unwrap_or(value.enabled),
                    "corner" => {
                        if let Some(corner) = Corner::ALL.into_iter().find(|c| c.name() == raw) {
                            value.corner = corner;
                        }
                    }
                    "size" => {
                        if let Ok(size) = raw.parse::<u16>()
                            && super::SIZES.contains(&size)
                        {
                            value.size_px = size;
                        }
                    }
                    "zoom" => {
                        if let Ok(zoom) = raw.parse::<f32>()
                            && super::ZOOMS.contains(&zoom)
                        {
                            value.blocks_per_pixel = zoom;
                        }
                    }
                    "rotate" => {
                        value.rotate_with_player = raw.parse().unwrap_or(value.rotate_with_player)
                    }
                    "shape" => match raw {
                        "round" => value.round = true,
                        "square" => value.round = false,
                        _ => {}
                    },
                    _ => {}
                }
            }
            value
        }

        pub fn encode(self) -> String {
            format!(
                "# Minimap Reworked settings\nenabled={}\ncorner={}\nsize={}\nshape={}\nzoom={}\nrotate={}\nmini_dimension={}\nworld_dimension={}\nsmooth_worldmap={}\noverworld_coordinates={}\nnether_coordinates={}\n",
                self.enabled,
                self.corner.name(),
                self.size_px,
                if self.round { "round" } else { "square" },
                self.blocks_per_pixel,
                self.rotate_with_player,
                self.mini_dimension.name(),
                self.world_dimension.name(),
                self.smooth_worldmap,
                self.overworld_coordinates,
                self.nether_coordinates,
            )
        }

        // Returns the changed field mask. Unknown buttons are harmless.
        fn click(&mut self, button: usize) -> u16 {
            match button {
                0 => self.enabled = !self.enabled,
                1 => {
                    let index = Corner::ALL
                        .iter()
                        .position(|c| *c == self.corner)
                        .unwrap_or(1);
                    self.corner = Corner::ALL[(index + 1) % Corner::ALL.len()];
                }
                2 => self.size_px = super::cycle(self.size_px, &super::SIZES),
                3 => self.round = !self.round,
                4 => self.blocks_per_pixel = super::cycle(self.blocks_per_pixel, &super::ZOOMS),
                5 => self.rotate_with_player = !self.rotate_with_player,
                6 => {
                    *self = Self::default();
                    return 0xfbf;
                }
                7 => {
                    self.mini_dimension = super::cycle(self.mini_dimension, &super::Dimension::ALL)
                }
                8 => {
                    self.world_dimension =
                        super::cycle(self.world_dimension, &super::Dimension::ALL)
                }
                9 => self.smooth_worldmap = !self.smooth_worldmap,
                10 => self.overworld_coordinates = !self.overworld_coordinates,
                11 => self.nether_coordinates = !self.nether_coordinates,
                _ => return 0,
            }
            1 << button
        }

        // Only locally edited fields override settings loaded asynchronously.
        fn overlay(&mut self, edited: Self, mask: u16) {
            if mask & 1 != 0 {
                self.enabled = edited.enabled;
            }
            if mask & 2 != 0 {
                self.corner = edited.corner;
            }
            if mask & 4 != 0 {
                self.size_px = edited.size_px;
            }
            if mask & 8 != 0 {
                self.round = edited.round;
            }
            if mask & 16 != 0 {
                self.blocks_per_pixel = edited.blocks_per_pixel;
            }
            if mask & 128 != 0 {
                self.mini_dimension = edited.mini_dimension;
            }
            if mask & 256 != 0 {
                self.world_dimension = edited.world_dimension;
            }
            if mask & 512 != 0 {
                self.smooth_worldmap = edited.smooth_worldmap;
            }
            if mask & 1024 != 0 {
                self.overworld_coordinates = edited.overworld_coordinates;
            }
            if mask & 2048 != 0 {
                self.nether_coordinates = edited.nether_coordinates;
            }
            if mask & 32 != 0 {
                self.rotate_with_player = edited.rotate_with_player;
            }
        }

        pub fn labels(self) -> Vec<String> {
            let on = |v| {
                if v {
                    tr("On", "Вкл", "Activé", "Activado", "Ein")
                } else {
                    tr("Off", "Выкл", "Désactivé", "Desactivado", "Aus")
                }
            };
            vec![
                localized_format!(
                    "Minimap: {}",
                    "Миникарта: {}",
                    "Minicarte : {}",
                    "Minimapa: {}",
                    "Minikarte: {}",
                    on(self.enabled)
                ),
                localized_format!(
                    "Position: {}",
                    "Положение: {}",
                    "Position : {}",
                    "Posición: {}",
                    "Position: {}",
                    self.corner.label()
                ),
                localized_format!(
                    "Size: {} GUI px",
                    "Размер: {} GUI px",
                    "Taille : {} px d’interface",
                    "Tamaño: {} px de interfaz",
                    "Größe: {} GUI-Pixel",
                    self.size_px
                ),
                localized_format!(
                    "Shape: {}",
                    "Форма: {}",
                    "Forme : {}",
                    "Forma: {}",
                    "Form: {}",
                    if self.round {
                        tr("Circle", "Круг", "Cercle", "Círculo", "Kreis")
                    } else {
                        tr("Square", "Квадрат", "Carré", "Cuadrado", "Quadrat")
                    }
                ),
                localized_format!(
                    "Scale: {} blocks/pixel",
                    "Масштаб: {} бл./пикс.",
                    "Échelle : {} blocs/pixel",
                    "Escala: {} bloques/píxel",
                    "Maßstab: {} Blöcke/Pixel",
                    self.blocks_per_pixel
                ),
                localized_format!(
                    "Rotation: {}",
                    "Вращение: {}",
                    "Rotation : {}",
                    "Rotación: {}",
                    "Drehung: {}",
                    on(self.rotate_with_player)
                ),
                tr(
                    "Reset map settings",
                    "Сбросить настройки карты",
                    "Réinitialiser les réglages de la carte",
                    "Restablecer ajustes del mapa",
                    "Karteneinstellungen zurücksetzen",
                )
                .into(),
                localized_format!(
                    "Minimap dimension: {}",
                    "Измерение миникарты: {}",
                    "Dimension de la minicarte : {}",
                    "Dimensión del minimapa: {}",
                    "Minikarten-Dimension: {}",
                    self.mini_dimension.label()
                ),
                localized_format!(
                    "World map dimension: {}",
                    "Измерение большой карты: {}",
                    "Dimension de la carte du monde : {}",
                    "Dimensión del mapa del mundo: {}",
                    "Weltkarten-Dimension: {}",
                    self.world_dimension.label()
                ),
                localized_format!(
                    "World map — distant smoothing: {}",
                    "Большая карта — сглаживание вдали: {}",
                    "Carte du monde — lissage à distance : {}",
                    "Mapa del mundo — suavizado a distancia: {}",
                    "Weltkarte — Glättung in der Ferne: {}",
                    on(self.smooth_worldmap)
                ),
                localized_format!(
                    "Overworld coordinates: {}",
                    "Координаты Верхнего мира: {}",
                    "Coordonnées de la Surface : {}",
                    "Coordenadas del mundo normal: {}",
                    "Oberwelt-Koordinaten: {}",
                    on(self.overworld_coordinates)
                ),
                localized_format!(
                    "Nether coordinates: {}",
                    "Координаты Незера: {}",
                    "Coordonnées du Nether : {}",
                    "Coordenadas del Nether: {}",
                    "Nether-Koordinaten: {}",
                    on(self.nether_coordinates)
                ),
            ]
        }

        pub fn fitted_size(self, width: f32, height: f32) -> Option<f32> {
            // Horizon clamps its minimap to at least 16 GUI pixels. Hide if that cannot fit.
            let available = (width - 16.0).min(height - 64.0);
            (available >= 16.0).then(|| f32::from(self.size_px).min(available))
        }
    }

    pub(super) fn apply(settings: &mut Settings, button: usize) -> u16 {
        settings.click(button)
    }
    pub(super) fn merge(settings: &mut Settings, edited: Settings, mask: u16) {
        settings.overlay(edited, mask);
    }
}

use minimap::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiomeSymbol {
    Unknown,
    Water,
    Desert,
    Snow,
    Forest,
    Swamp,
    Mushroom,
    Cave,
    Badlands,
    Nether,
    End,
    Plains,
}

/// Built-in biome families use original pictograms; unknown/custom biomes stay explicit.
pub fn biome_symbol(id: Option<&str>) -> BiomeSymbol {
    use BiomeSymbol::*;
    let Some(name) = id.and_then(|id| id.strip_prefix("minecraft:")) else {
        return Unknown;
    };
    if name.contains("ocean") || name.contains("river") || name.contains("beach") {
        Water
    } else if name.contains("snow")
        || name.contains("frozen")
        || name.contains("ice")
        || name.contains("peaks")
        || name == "grove"
    {
        Snow
    } else if name.contains("swamp") {
        Swamp
    } else if name.contains("mushroom") {
        Mushroom
    } else if name.contains("badlands") {
        Badlands
    } else if name.contains("desert") {
        Desert
    } else if [
        "nether_wastes",
        "soul_sand_valley",
        "basalt_deltas",
        "crimson_forest",
        "warped_forest",
    ]
    .contains(&name)
    {
        Nether
    } else if name == "the_end" || name.starts_with("end_") || name == "small_end_islands" {
        End
    } else if name.contains("forest")
        || name.contains("taiga")
        || name.contains("jungle")
        || name.contains("cherry")
        || name == "pale_garden"
    {
        Forest
    } else if ["dripstone_caves", "lush_caves", "deep_dark"].contains(&name) {
        Cave
    } else if [
        "plains",
        "sunflower_plains",
        "savanna",
        "savanna_plateau",
        "windswept_savanna",
        "meadow",
        "windswept_hills",
        "windswept_gravelly_hills",
        "stony_shore",
    ]
    .contains(&name)
    {
        Plains
    } else {
        Unknown
    }
}

/// Portal-scale coordinates. Scale continuous X/Z before flooring, including
/// negative coordinates; Y is unchanged. The End has no such portal mapping.
pub fn portal_coordinates(from: Dimension, to: Dimension, at: [f64; 3]) -> Option<[i64; 3]> {
    if !matches!(from, Dimension::Overworld | Dimension::Nether)
        || !matches!(to, Dimension::Overworld | Dimension::Nether)
        || !at.iter().all(|v| v.is_finite())
    {
        return None;
    }
    let [x, z] = from.convert(to, [at[0], at[2]]);
    Some([x.floor() as i64, at[1].floor() as i64, z.floor() as i64])
}

pub fn coordinate_lines(settings: Settings, dimension: Dimension, at: [f64; 3]) -> Vec<String> {
    [
        (settings.overworld_coordinates, Dimension::Overworld),
        (settings.nether_coordinates, Dimension::Nether),
    ]
    .into_iter()
    .filter(|(enabled, _)| *enabled)
    .filter_map(|(_, target)| {
        portal_coordinates(dimension, target, at)
            .map(|[x, y, z]| format!("{}: {x} {y} {z}", target.label()))
    })
    .collect()
}

fn cycle<T: Copy + PartialEq>(current: T, choices: &[T]) -> T {
    let index = choices.iter().position(|v| *v == current).unwrap_or(0);
    choices[(index + 1) % choices.len()]
}

pub const MAX_WAYPOINTS: usize = 64;
const MAX_TOTAL_WAYPOINTS: usize = 4096;

/// Stable, namespaced identity supplied by Horizon, never a session counter or
/// Debug representation. Unknown identities must not read or mutate waypoints.
pub fn waypoint_world(singleplayer: bool, identity: &str) -> Option<String> {
    if identity.is_empty() || identity.len() > 1024 || identity.chars().any(char::is_control) {
        return None;
    }
    Some(format!(
        "{}:{identity}",
        if singleplayer { "save" } else { "server" }
    ))
}
fn valid_waypoint_world(world: &str) -> bool {
    world
        .strip_prefix("save:")
        .or_else(|| world.strip_prefix("server:"))
        .is_some_and(|id| !id.is_empty() && id.len() <= 1024 && !id.chars().any(char::is_control))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScreenRequest {
    Map,
    Waypoints,
}

static SCREEN_REQUEST: AtomicU8 = AtomicU8::new(0);
static TEXT_INPUT: AtomicU8 = AtomicU8::new(0);

/// The key hooks only set a request; the client module, which owns the
/// screen mode, consumes it on the next frame. This also keeps the key helper
/// usable in a bundle without the client module.
pub fn request_screen(request: ScreenRequest) {
    SCREEN_REQUEST.store(
        match request {
            ScreenRequest::Map => 1,
            ScreenRequest::Waypoints => 2,
        },
        Ordering::Release,
    );
}

pub fn take_screen_request() -> Option<ScreenRequest> {
    match SCREEN_REQUEST.swap(0, Ordering::AcqRel) {
        1 => Some(ScreenRequest::Map),
        2 => Some(ScreenRequest::Waypoints),
        _ => None,
    }
}

/// While a waypoint name is being typed, the global map/waypoint keys must
/// not close the screen when the name contains that letter.
pub fn set_text_input(active: bool) {
    TEXT_INPUT.store(u8::from(active), Ordering::Release);
}

pub fn text_input_active() -> bool {
    TEXT_INPUT.load(Ordering::Acquire) != 0
}
const MAX_WAYPOINT_NAME: usize = 32;

/// A point on a map: the dimension it belongs to, its block coordinates and
/// the name the player gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waypoint {
    pub world: String,
    pub dimension: Dimension,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub name: String,
}

impl Waypoint {
    /// Trims and shortens a name for the settings file and the markers.
    pub fn clean_name(name: &str) -> String {
        let name: String = name
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_WAYPOINT_NAME)
            .collect();
        let name = name.trim();
        if name.is_empty() {
            tr("Waypoint", "Вейпоинт", "Repère", "Marcador", "Wegpunkt").into()
        } else {
            name.to_owned()
        }
    }

    fn decode(text: &str) -> Option<Self> {
        let mut parts = text.strip_prefix("waypoint_v2=")?.splitn(6, ':');
        let world = unescape_name(parts.next()?);
        if !valid_waypoint_world(&world) {
            return None;
        }
        let dimension = parts.next()?;
        let dimension = Dimension::ALL
            .into_iter()
            .find(|candidate| candidate.name() == dimension)?;
        if dimension == Dimension::Player {
            return None;
        }
        Some(Self {
            world,
            dimension,
            x: parts.next()?.parse().ok()?,
            y: parts.next()?.parse().ok()?,
            z: parts.next()?.parse().ok()?,
            name: Self::clean_name(&unescape_name(parts.next().unwrap_or(""))),
        })
    }

    fn encode(&self) -> String {
        format!(
            "waypoint_v2={}:{}:{}:{}:{}:{}\n",
            escape_name(&self.world),
            self.dimension.name(),
            self.x,
            self.y,
            self.z,
            escape_name(&self.name)
        )
    }
}

// The file is one line per waypoint: `%`, `:`, CR and LF are escaped so the
// name can hold spaces, Cyrillic and most punctuation.
fn escape_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        match c {
            '%' => out.push_str("%25"),
            ':' => out.push_str("%3A"),
            '\n' => out.push_str("%0A"),
            '\r' => out.push_str("%0D"),
            _ => out.push(c),
        }
    }
    out
}

fn unescape_name(name: &str) -> String {
    let mut out = String::new();
    let mut chars = name.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let hex: String = chars.by_ref().take(2).collect();
        match hex.as_str() {
            "25" => out.push('%'),
            "3A" => out.push(':'),
            "0A" => out.push('\n'),
            "0D" => out.push('\r'),
            _ => {
                out.push('%');
                out.push_str(&hex);
            }
        }
    }
    out
}

fn decode_waypoints(text: &str) -> Vec<Waypoint> {
    let mut waypoints = Vec::new();
    for line in text.lines() {
        if let Some(waypoint) = Waypoint::decode(line.trim()) {
            push_waypoint(&mut waypoints, waypoint);
        }
    }
    waypoints
}

/// Adds a waypoint once and keeps the list bounded; false when it was a
/// duplicate with the same name. A different name at the same place renames it.
fn push_waypoint(waypoints: &mut Vec<Waypoint>, waypoint: Waypoint) -> bool {
    if let Some(existing) = waypoints.iter_mut().find(|existing| {
        existing.world == waypoint.world
            && existing.dimension == waypoint.dimension
            && existing.x == waypoint.x
            && existing.z == waypoint.z
    }) {
        if existing.name == waypoint.name {
            return false;
        }
        existing.name = waypoint.name;
        return true;
    }
    if waypoints
        .iter()
        .filter(|w| w.world == waypoint.world)
        .count()
        >= MAX_WAYPOINTS
    {
        let oldest = waypoints
            .iter()
            .position(|w| w.world == waypoint.world)
            .unwrap();
        waypoints.remove(oldest);
    } else if waypoints.len() >= MAX_TOTAL_WAYPOINTS {
        // Never evict another world's data to make room in this world.
        return false;
    }
    waypoints.push(waypoint);
    true
}

#[derive(Default)]
struct State {
    settings: Settings,
    waypoints: Vec<Waypoint>,
    touched: u16,
    revision: u64,
    error: Option<String>,
}

// One bounded notification queue and one settings worker; no filesystem access in HUD callbacks.
pub struct Controller {
    state: Arc<Mutex<State>>,
    notify: Mutex<Option<mpsc::SyncSender<()>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    pub maps: maps::Service,
}

impl Controller {
    pub fn open(directory: PathBuf) -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let (tx, rx) = mpsc::sync_channel(1);
        let shared = Arc::clone(&state);
        let worker = thread::Builder::new()
            .name("minimap-settings".into())
            .spawn(move || {
                settings_worker(&directory, &shared, rx);
            });
        let (notify, worker) = match worker {
            Ok(worker) => (Some(tx), Some(worker)),
            Err(error) => {
                state.lock().unwrap_or_else(|e| e.into_inner()).error = Some(error.to_string());
                (None, None)
            }
        };
        Self {
            state,
            notify: Mutex::new(notify),
            worker: Mutex::new(worker),
            maps: maps::Service::new(),
        }
    }

    pub fn settings(&self) -> Settings {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .settings
    }

    pub fn labels(&self) -> Vec<String> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut labels = state.settings.labels();
        if state.error.is_some() {
            labels.push(
                tr(
                    "Failed to save settings: see the client log",
                    "Ошибка сохранения настроек: см. журнал клиента",
                    "Échec de l’enregistrement des réglages : voir le journal du client",
                    "Error al guardar los ajustes: consulta el registro del cliente",
                    "Einstellungen konnten nicht gespeichert werden: siehe Client-Protokoll",
                )
                .into(),
            );
        }
        labels
    }

    pub fn click(&self, button: usize) {
        let notify = self.notify.lock().unwrap_or_else(|e| e.into_inner());
        let Some(notify) = notify.as_ref() else {
            return;
        };
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let changed = minimap::apply(&mut state.settings, button);
        if changed == 0 {
            return;
        }
        state.touched |= changed;
        state.revision += 1;
        drop(state);
        // Full means a notification is already queued; the worker always reads the latest state.
        if let Err(mpsc::TrySendError::Disconnected(_)) = notify.try_send(()) {
            self.state.lock().unwrap_or_else(|e| e.into_inner()).error =
                Some("Settings worker stopped".into());
        }
    }

    pub fn choose_dimension(&self, large: bool, dimension: Dimension) {
        let notify = self.notify.lock().unwrap_or_else(|e| e.into_inner());
        let Some(notify) = notify.as_ref() else {
            return;
        };
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if large {
            state.settings.world_dimension = dimension;
            state.touched |= 256;
        } else {
            state.settings.mini_dimension = dimension;
            state.touched |= 128;
        }
        state.revision += 1;
        drop(state);
        let _ = notify.try_send(());
    }

    pub fn waypoints(&self, world: &str) -> Vec<Waypoint> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .waypoints
            .iter()
            .filter(|w| w.world == world)
            .cloned()
            .collect()
    }

    /// Adds a waypoint and persists it with the settings; false for a duplicate.
    pub fn add_waypoint(&self, mut waypoint: Waypoint) -> bool {
        if !valid_waypoint_world(&waypoint.world) || waypoint.dimension == Dimension::Player {
            return false;
        }
        waypoint.name = Waypoint::clean_name(&waypoint.name);
        let notify = self.notify.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !push_waypoint(&mut state.waypoints, waypoint) {
            return false;
        }
        state.revision += 1;
        drop(state);
        if let Some(notify) = notify.as_ref()
            && let Err(mpsc::TrySendError::Disconnected(_)) = notify.try_send(())
        {
            self.state.lock().unwrap_or_else(|e| e.into_inner()).error =
                Some("Settings worker stopped".into());
        }
        true
    }

    pub fn remove_waypoint(&self, world: &str, dimension: Dimension, x: i32, z: i32) -> bool {
        let notify = self.notify.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let before = state.waypoints.len();
        state.waypoints.retain(|waypoint| {
            !(waypoint.world == world
                && waypoint.dimension == dimension
                && waypoint.x == x
                && waypoint.z == z)
        });
        if state.waypoints.len() == before {
            return false;
        }
        state.revision += 1;
        drop(state);
        if let Some(notify) = notify.as_ref() {
            let _ = notify.try_send(());
        }
        true
    }

    pub fn rename_waypoint(
        &self,
        world: &str,
        dimension: Dimension,
        x: i32,
        z: i32,
        name: &str,
    ) -> bool {
        let name = Waypoint::clean_name(name);
        let notify = self.notify.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(waypoint) = state.waypoints.iter_mut().find(|waypoint| {
            waypoint.world == world
                && waypoint.dimension == dimension
                && waypoint.x == x
                && waypoint.z == z
        }) else {
            return false;
        };
        if waypoint.name == name {
            return false;
        }
        waypoint.name = name;
        state.revision += 1;
        drop(state);
        if let Some(notify) = notify.as_ref() {
            let _ = notify.try_send(());
        }
        true
    }

    // Called on application exit, never during drawing. Drain accepted edits before returning.
    pub fn shutdown(&self) {
        self.maps.shutdown();
        self.notify.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(worker) = self.worker.lock().unwrap_or_else(|e| e.into_inner()).take()
            && worker.join().is_err()
        {
            eprintln!("[{MOD_ID}] Settings worker panicked");
        }
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn settings_worker(directory: &Path, shared: &Mutex<State>, rx: mpsc::Receiver<()>) {
    let path = directory.join(SETTINGS_FILE);
    let loaded = read_profile_migration(&path);
    {
        let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
        match loaded {
            Ok((mut settings, loaded, discarded_legacy)) => {
                if discarded_legacy {
                    state.revision += 1;
                }
                minimap::merge(&mut settings, state.settings, state.touched);
                // Waypoints added while the file was read stay: merge them over
                // the loaded list rather than discarding either side.
                let early = std::mem::take(&mut state.waypoints);
                state.waypoints = loaded;
                for waypoint in early {
                    push_waypoint(&mut state.waypoints, waypoint);
                }
                state.settings = settings;
            }
            Err(error) => {
                eprintln!("[{MOD_ID}] Cannot read {}: {error}", path.display());
                state.error = Some(error.to_string());
            }
        }
    }
    let mut saved_revision = 0;
    persist_latest(&path, shared, &mut saved_revision);
    while rx.recv().is_ok() {
        persist_latest(&path, shared, &mut saved_revision);
    }
    // Retry a dirty last revision once at normal exit after a transient failure.
    persist_latest(&path, shared, &mut saved_revision);
}

fn persist_latest(path: &Path, shared: &Mutex<State>, saved_revision: &mut u64) {
    let (settings, waypoints, revision) = {
        let state = shared.lock().unwrap_or_else(|e| e.into_inner());
        (state.settings, state.waypoints.clone(), state.revision)
    };
    if revision <= *saved_revision {
        return;
    }
    let result = write_profile(path, settings, &waypoints, revision);
    if result.is_ok() {
        *saved_revision = revision;
    }
    shared.lock().unwrap_or_else(|e| e.into_inner()).error = result.err().map(|error| {
        eprintln!("[{MOD_ID}] Cannot save {}: {error}", path.display());
        format!("Save: {error}")
    });
}

#[cfg(test)]
fn read_profile(path: &Path) -> io::Result<(Settings, Vec<Waypoint>)> {
    read_profile_migration(path).map(|(settings, points, _)| (settings, points))
}
fn read_profile_migration(path: &Path) -> io::Result<(Settings, Vec<Waypoint>, bool)> {
    const LIMIT: usize = 16 * 1024 * 1024;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((Settings::default(), Vec::new(), false));
        }
        Err(error) => return Err(error),
    };
    let mut text = String::new();
    file.take(LIMIT as u64 + 1).read_to_string(&mut text)?;
    if text.len() > LIMIT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Settings file exceeds 16 MiB",
        ));
    }
    Ok((
        Settings::decode(&text),
        decode_waypoints(&text),
        text.lines()
            .any(|line| line.trim().starts_with("waypoint=")),
    ))
}

#[cfg(test)]
fn read_settings(path: &Path) -> io::Result<Settings> {
    read_profile(path).map(|profile| profile.0)
}

fn write_profile(
    path: &Path,
    settings: Settings,
    waypoints: &[Waypoint],
    revision: u64,
) -> io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::other("No settings directory"))?;
    fs::create_dir_all(directory)?;
    // Temporary preferences file, not a world cache. Concurrent clients cannot tear the final file.
    let temporary = directory.join(format!(".settings-{}-{revision}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        let mut text = settings.encode();
        for waypoint in waypoints {
            text.push_str(&waypoint.encode());
        }
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

static CONTROLLER: OnceLock<Controller> = OnceLock::new();

pub fn initialize(directory: PathBuf) -> &'static Controller {
    CONTROLLER.get_or_init(|| Controller::open(directory))
}

pub fn shutdown() {
    if let Some(controller) = CONTROLLER.get() {
        controller.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

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

    fn point(world: &str, name: &str) -> Waypoint {
        Waypoint {
            world: world.into(),
            dimension: Dimension::Overworld,
            x: 10,
            y: 64,
            z: -5,
            name: name.into(),
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
        assert!(controller.rename_waypoint(&b, Dimension::Overworld, 10, -5, "B renamed"));
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
        assert!(!controller.settings().smooth_worldmap);
        assert_eq!(controller.waypoints(&scoped.world), vec![scoped.clone()]);
        assert!(controller.waypoints("save:A").is_empty());
        let file = fs::read_to_string(directory.join(SETTINGS_FILE)).unwrap();
        assert!(!file.lines().any(|l| l.starts_with("waypoint=")));
        assert!(file.contains("waypoint_v2="));
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
    fn waypoint_names_roundtrip_and_waypoints_can_be_removed() {
        let directory = scratch();
        let waypoint = Waypoint {
            world: "save:test".into(),
            dimension: Dimension::Overworld,
            x: 1,
            y: 2,
            z: 3,
            name: "Дом: у % реки".into(),
        };
        assert_eq!(
            Waypoint::decode(waypoint.encode().trim()).unwrap(),
            waypoint
        );
        let controller = Controller::open(directory.clone());
        assert!(controller.add_waypoint(waypoint));
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while controller.waypoints("save:test").is_empty() {
            assert!(std::time::Instant::now() < until);
            thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(controller.rename_waypoint("save:test", Dimension::Overworld, 1, 3, "Новое имя"));
        controller.shutdown();
        let reopened = Controller::open(directory.clone());
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while reopened.waypoints("save:test").is_empty() {
            assert!(std::time::Instant::now() < until);
            thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(reopened.waypoints("save:test")[0].name, "Новое имя");
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

// Shared in-memory terrain repository; retention is temporarily uncapped until the container.
// No game objects cross the worker boundary.
pub mod maps {
    use super::Dimension;
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::{
            Arc, Mutex,
            atomic::{AtomicU64, Ordering},
            mpsc,
        },
        thread::{self, JoinHandle},
    };

    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub enum Layer {
        Surface,
        Below(i32),
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub enum Portal {
        Nether,
        End,
        Gateway,
    }
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct Block {
        pub rgb: u32,
        pub air: bool,
        /// How much this block's colour covers what is below it, 0..=255.
        /// 0 is an ordinary opaque block; water and glass carry their tint
        /// over the block underneath instead of hiding it.
        pub alpha: u8,
        /// Ground cover that the map leaves out, such as grass and ferns:
        /// the terrain below it is shown instead.
        pub hidden: bool,
        /// A water block, whose tint thickens with the depth of the column.
        pub water: bool,
        /// A portal block, recorded as a point on the map.
        pub portal: Option<Portal>,
    }
    #[derive(Clone)]
    pub struct Tile {
        pub pixels: Vec<u8>,
        /// The visible surface height of each column, for relief shading
        /// across chunk edges.
        pub heights: [i32; 256],
        /// Whether the column's visible block is the cut plane of an
        /// underground layer (`Layer::Below`).
        pub walls: [bool; 256],
        /// Portal blocks met while scanning the columns.
        pub portals: Vec<([i32; 3], Portal)>,
        pub biomes: Vec<Option<Arc<str>>>,
    }
    impl Default for Tile {
        fn default() -> Self {
            Self {
                pixels: Vec::new(),
                heights: [0; 256],
                walls: [false; 256],
                portals: Vec::new(),
                biomes: Vec::new(),
            }
        }
    }
    /// The most translucent layers one column composites. A deep ocean is one
    /// water layer, not one per block, but glass above water above ice still
    /// blends in order.
    const TRANSLUCENT_LAYERS: usize = 8;

    fn blend(base: u32, overlay: u32, alpha: u8) -> u32 {
        let alpha = u32::from(alpha);
        let channel = |shift: u32| {
            ((((base >> shift) & 255) * (255 - alpha) + ((overlay >> shift) & 255) * alpha) / 255)
                & 255
        };
        (channel(16) << 16) | (channel(8) << 8) | channel(0)
    }

    /// The map colour of a block after its biome tint: a grass block's top is
    /// drawn with the biome's grass colour, as the world tints it, while every
    /// other block keeps its own map colour. Unknown biomes keep `base`.
    pub fn block_map_color(name: &str, base: u32, grass: Option<u32>) -> u32 {
        match (name, grass) {
            ("minecraft:grass_block" | "minecraft:grass", Some(grass)) => grass & 0x00ff_ffff,
            _ => base,
        }
    }

    /// A shallow seabed keeps most of its colour; deep water darkens it in
    /// steps instead of looking one block deep everywhere.
    fn water_alpha(depth: u32) -> u8 {
        (70 + depth.saturating_mul(10)).min(190) as u8
    }

    pub fn sample(
        mut block: impl FnMut(i32, i32, i32) -> Block,
        top: impl Fn(i32, i32) -> i32,
        chunk: [i32; 2],
        minimum: i32,
        layer: Layer,
    ) -> Tile {
        let mut tile = Tile {
            pixels: vec![0; 16 * 16 * 4],
            heights: [minimum; 256],
            walls: [false; 256],
            portals: Vec::new(),
            biomes: Vec::new(),
        };
        for z in 0..16 {
            for x in 0..16 {
                let wx = chunk[0] * 16 + x;
                let wz = chunk[1] * 16 + z;
                let ceiling = match layer {
                    Layer::Surface => top(wx, wz) - 1,
                    Layer::Below(y) => y.min(top(wx, wz) - 1),
                };
                let i = (z * 16 + x) as usize;
                let mut top_y = None;
                let mut overlays: [Option<(u32, u8)>; TRANSLUCENT_LAYERS] =
                    [None; TRANSLUCENT_LAYERS];
                let mut layer_count = 0;
                let mut base = None;
                let mut base_y = None;
                let mut water_depth = 0u32;
                let mut water_layer = None;
                for y in (minimum..=ceiling).rev() {
                    let b = block(wx, y, wz);
                    if b.air || b.hidden {
                        continue;
                    }
                    if let Some(kind) = b.portal
                        && !tile
                            .portals
                            .iter()
                            .any(|(at, existing)| at[0] == wx && at[2] == wz && *existing == kind)
                    {
                        tile.portals.push(([wx, y, wz], kind));
                    }
                    if top_y.is_none() {
                        top_y = Some(y);
                    }
                    if b.water {
                        water_depth += 1;
                    }
                    if b.alpha > 0 && b.rgb != 0 {
                        // One layer per colour in a row: consecutive water
                        // blocks are the same surface, not stacked tints.
                        let distinct = layer_count == 0
                            || (layer_count < TRANSLUCENT_LAYERS
                                && overlays[layer_count - 1].is_some_and(|(rgb, _)| rgb != b.rgb));
                        if distinct && layer_count < TRANSLUCENT_LAYERS {
                            if b.water && water_layer.is_none() {
                                water_layer = Some(layer_count);
                            }
                            overlays[layer_count] = Some((b.rgb, b.alpha));
                            layer_count += 1;
                        } else if b.water && water_layer.is_none() && layer_count > 0 {
                            water_layer = Some(layer_count - 1);
                        }
                        continue;
                    }
                    if b.rgb == 0 {
                        continue;
                    }
                    base = Some(b.rgb);
                    base_y = Some(y);
                    break;
                }
                // Deep water covers the floor more than shallow water, so a
                // seabed is not flattened to "one block deep" everywhere.
                if let Some(index) = water_layer
                    && water_depth > 0
                    && let Some((rgb, _)) = overlays[index]
                {
                    overlays[index] = Some((rgb, water_alpha(water_depth)));
                }
                let color = match base {
                    Some(base) => overlays[..layer_count]
                        .iter()
                        .rev()
                        .flatten()
                        .fold(base, |color, (rgb, alpha)| blend(color, *rgb, *alpha)),
                    // No floor in the sampled column (open ocean): the lowest
                    // overlay is the surface instead of an unknown pixel.
                    None if layer_count > 0 => overlays[layer_count - 1]
                        .map(|(rgb, _)| rgb)
                        .unwrap_or_default(),
                    None => continue,
                };
                // Relief follows the visible floor, not a flat water surface:
                // the seabed keeps its shape under the tinted water.
                let visible_y = base_y.or(top_y).unwrap_or(minimum);
                tile.heights[i] = visible_y;
                tile.walls[i] = matches!(
                    (layer, base_y.or(top_y)),
                    (Layer::Below(c), Some(y)) if c == y
                );
                let rgb = ((color >> 16) & 255) as u8;
                let green = ((color >> 8) & 255) as u8;
                let blue = (color & 255) as u8;
                tile.pixels[i * 4] = rgb;
                tile.pixels[i * 4 + 1] = green;
                tile.pixels[i * 4 + 2] = blue;
                tile.pixels[i * 4 + 3] = 255;
            }
        }
        tile
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct View {
        pub session: u64,
        pub dimension: Dimension,
        pub layer: Layer,
        pub center: [f64; 2],
        pub span: [f64; 2],
        pub width: usize,
        pub height: usize,
        pub angle: f64,
        pub round: bool,
        pub large: bool,
        pub smooth: bool,
    }
    impl View {
        pub fn lod(&self) -> usize {
            if !self.smooth {
                return 0;
            }
            let blocks = (self.span[0] / self.width.max(1) as f64)
                .max(self.span[1] / self.height.max(1) as f64);
            blocks.log2().floor().clamp(0.0, 4.0) as usize
        }

        fn atlas_region(&self) -> [i64; 4] {
            let step = 2f64.powf(
                (self.span[0].max(self.span[1]) / 4.0)
                    .max(64.0)
                    .log2()
                    .ceil(),
            );
            let origin = self.center.map(|v| (v / step).round() * step);
            let radius = self.span.map(|v| (v / 2.0 / step).ceil() * step + step);
            [
                (origin[0] - radius[0]) as i64,
                (origin[1] - radius[1]) as i64,
                (radius[0] * 2.0) as i64,
                (radius[1] * 2.0) as i64,
            ]
        }
        pub fn demands(&self, dimension: Dimension, layer: Layer, chunk: [i32; 2]) -> bool {
            if dimension != self.dimension || layer != self.layer {
                return false;
            }
            let cx = (self.center[0].floor() as i32).div_euclid(16);
            let cz = (self.center[1].floor() as i32).div_euclid(16);
            if !(cx - 32..cx + 32).contains(&chunk[0]) || !(cz - 32..cz + 32).contains(&chunk[1]) {
                return false;
            }
            let dx = f64::from(chunk[0]) * 16.0 + 8.0 - self.center[0];
            let dz = f64::from(chunk[1]) * 16.0 + 8.0 - self.center[1];
            let (sn, cs) = self.angle.sin_cos();
            let margin = 8.0 * (sn.abs() + cs.abs());
            (dx * cs + dz * sn).abs() <= self.span[0] / 2.0 + margin
                && (-dx * sn + dz * cs).abs() <= self.span[1] / 2.0 + margin
        }
        pub fn world_at(&self, x: f64, y: f64) -> [f64; 2] {
            let dx = (x / self.width as f64 - 0.5) * self.span[0];
            let dz = (y / self.height as f64 - 0.5) * self.span[1];
            let (s, c) = self.angle.sin_cos();
            [
                self.center[0] + dx * c - dz * s,
                self.center[1] + dx * s + dz * c,
            ]
        }
        /// The block under pixel `(x, y)` of the frame (`world_at`'s inverse,
        /// rounded down).
        pub fn block_at(&self, x: f64, y: f64) -> [i32; 2] {
            let world = self.world_at(x, y);
            [world[0].floor() as i32, world[1].floor() as i32]
        }
        /// Minecraft yaw projected through this view into GUI coordinates.
        pub fn player_heading(&self, yaw: f64, size: [f32; 2]) -> [f32; 2] {
            let angle = self.angle - yaw.to_radians();
            let dx = angle.sin() * f64::from(size[0]) / self.span[0];
            let dy = angle.cos() * f64::from(size[1]) / self.span[1];
            let length = dx.hypot(dy);
            [(dx / length) as f32, (dy / length) as f32]
        }

        /// The frame pixel of a world position: `world_at`'s inverse.
        pub fn screen_at(&self, [world_x, world_z]: [f64; 2]) -> [f64; 2] {
            let (s, c) = self.angle.sin_cos();
            let dx = world_x - self.center[0];
            let dz = world_z - self.center[1];
            let flat_x = dx * c + dz * s;
            let flat_z = -dx * s + dz * c;
            [
                (flat_x / self.span[0] + 0.5) * self.width as f64,
                (flat_z / self.span[1] + 0.5) * self.height as f64,
            ]
        }
    }
    const EASE_RATE: f64 = 24.0;
    const ZOOM_RATE: f64 = 18.0;
    const EASE_EPSILON: f64 = 0.01;
    const ZOOM_EPSILON: f64 = 0.0005;
    const SNAP_DISTANCE: f64 = 48.0;
    const MIN_ZOOM: f64 = 0.1;
    const MAX_ZOOM: f64 = 64.0;

    fn ease(current: f64, target: f64, dt: f64, rate: f64, epsilon: f64) -> f64 {
        let next = current + (target - current) * (1.0 - (-dt * rate).exp());
        if (target - next).abs() <= epsilon {
            target
        } else {
            next
        }
    }
    fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
    }
    /// Eases a displayed point toward a target at the same rate as [`Camera`].
    pub fn follow_point(current: [f64; 2], target: [f64; 2], dt: f64) -> [f64; 2] {
        if distance(current, target) > SNAP_DISTANCE {
            return target;
        }
        [
            ease(current[0], target[0], dt, EASE_RATE, EASE_EPSILON),
            ease(current[1], target[1], dt, EASE_RATE, EASE_EPSILON),
        ]
    }
    /// A zoom step of `delta` notches, clamped to the map's usable range.
    pub fn zoom_step(zoom: f64, delta: f64) -> f64 {
        if !delta.is_finite() {
            return zoom;
        }
        (zoom * 2f64.powf(-delta.clamp(-4.0, 4.0))).clamp(MIN_ZOOM, MAX_ZOOM)
    }

    /// The displayed camera of one map. Panning, zooming and following ease
    /// toward their target every frame, so mouse steps and the player's 20 Hz
    /// ticks do not jump. A zoom keeps the world point under the cursor fixed
    /// for the whole animation.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Camera {
        pub center: [f64; 2],
        pub target: [f64; 2],
        pub zoom: f64,
        pub zoom_target: f64,
        anchor: Option<([f64; 2], [f64; 2])>,
    }
    impl Camera {
        pub fn new(center: [f64; 2], zoom: f64) -> Self {
            Self {
                center,
                target: center,
                zoom: zoom.clamp(MIN_ZOOM, MAX_ZOOM),
                zoom_target: zoom.clamp(MIN_ZOOM, MAX_ZOOM),
                anchor: None,
            }
        }
        /// Jumps to a position: a world change, a dimension switch or a
        /// teleport, where easing would slide across the map.
        pub fn snap(&mut self, center: [f64; 2]) {
            self.center = center;
            self.target = center;
            self.anchor = None;
        }
        pub fn follow(&mut self, point: [f64; 2]) {
            if distance(self.center, point) > SNAP_DISTANCE {
                self.snap(point);
            } else {
                self.target = point;
            }
        }
        /// Drags the map by `delta` GUI pixels, one world pixel per GUI pixel.
        pub fn pan(&mut self, delta: [f64; 2]) {
            let shift = [delta[0] * self.zoom, delta[1] * self.zoom];
            match self.anchor.as_mut() {
                Some((_, anchor)) => {
                    anchor[0] -= shift[0];
                    anchor[1] -= shift[1];
                }
                None => {
                    self.target[0] -= shift[0];
                    self.target[1] -= shift[1];
                }
            }
        }
        /// Scrolls `delta` notches at `offset` GUI pixels from the map centre.
        pub fn zoom_at(&mut self, delta: f64, offset: [f64; 2]) {
            let next = zoom_step(self.zoom_target, delta);
            if next == self.zoom_target {
                return;
            }
            self.zoom_target = next;
            self.anchor = Some((
                offset,
                [
                    self.center[0] + offset[0] * self.zoom,
                    self.center[1] + offset[1] * self.zoom,
                ],
            ));
        }
        /// Advances the ease by `dt` seconds.
        pub fn advance(&mut self, dt: f64) {
            let dt = dt.clamp(0.0, 0.25);
            self.zoom = ease(self.zoom, self.zoom_target, dt, ZOOM_RATE, ZOOM_EPSILON);
            match self.anchor {
                Some((offset, anchor)) => {
                    self.center = [
                        anchor[0] - offset[0] * self.zoom,
                        anchor[1] - offset[1] * self.zoom,
                    ];
                    self.target = self.center;
                    if self.zoom == self.zoom_target {
                        self.anchor = None;
                    }
                }
                None => {
                    self.center = [
                        ease(self.center[0], self.target[0], dt, EASE_RATE, EASE_EPSILON),
                        ease(self.center[1], self.target[1], dt, EASE_RATE, EASE_EPSILON),
                    ];
                }
            }
        }
        /// Whether the view still has to move: a zoom or a pan in flight.
        /// Only the tests read it; a release build would warn it is unused.
        #[cfg(test)]
        pub fn moving(&self) -> bool {
            self.anchor.is_some() || self.zoom != self.zoom_target || self.center != self.target
        }
    }
    /// One real CPU LOD tile, always 16x16 samples; never expanded into fake chunks.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct LodKey {
        pub x: i32,
        pub z: i32,
        pub level: u8,
    }
    impl LodKey {
        pub fn size(self) -> i64 {
            16_i64 << self.level.min(22)
        }
        pub fn bounds(self) -> [i64; 4] {
            let size = self.size();
            [
                i64::from(self.x) * size,
                i64::from(self.z) * size,
                size,
                size,
            ]
        }
        pub fn intersects_known(self, mask: [u64; 4], rect: [i64; 4]) -> bool {
            let b = self.bounds();
            if !intersects(rect, b) {
                return false;
            }
            let stride = b[2] / 16;
            (0..256).any(|i| {
                mask[i / 64] & (1 << (i % 64)) != 0
                    && intersects(
                        rect,
                        [
                            b[0] + (i % 16) as i64 * stride,
                            b[1] + (i / 16) as i64 * stride,
                            stride,
                            stride,
                        ],
                    )
            })
        }
        pub fn cell(self, block: [i32; 2]) -> Option<usize> {
            let [x, z, size, _] = self.bounds();
            let (dx, dz) = (i64::from(block[0]) - x, i64::from(block[1]) - z);
            (dx >= 0 && dz >= 0 && dx < size && dz < size)
                .then(|| ((dz * 16 / size) * 16 + dx * 16 / size) as usize)
        }
    }
    #[derive(Clone, Default)]
    pub struct Coverage {
        pub session: u64,
        pub detailed: BTreeSet<(Dimension, Layer, i32, i32)>,
        pub lod: Vec<(Dimension, LodKey, [u64; 4])>,
        pub lod_data: BTreeMap<(Dimension, LodKey), Arc<Tile>>,
    }
    #[derive(Clone)]
    pub struct LodSample {
        pub key: LodKey,
        pub height: i32,
        pub biome: Option<Arc<str>>,
    }
    impl Coverage {
        pub fn lod_sample(
            &self,
            dimension: Dimension,
            layer: Layer,
            at: [i32; 2],
        ) -> Option<LodSample> {
            let key = self.lod_at(dimension, layer, at)?;
            let tile = self.lod_data.get(&(dimension, key))?;
            let cell = key.cell(at)?;
            Some(LodSample {
                key,
                height: tile.heights[cell],
                biome: tile.biomes.get(cell).cloned().flatten(),
            })
        }
        pub fn lod_at(&self, dimension: Dimension, layer: Layer, at: [i32; 2]) -> Option<LodKey> {
            if layer != Layer::Surface {
                return None;
            }
            for level in 0..=22 {
                let size = 16_i64 << level;
                let key = LodKey {
                    x: i64::from(at[0]).div_euclid(size) as i32,
                    z: i64::from(at[1]).div_euclid(size) as i32,
                    level,
                };
                if let Some(tile) = self.lod_data.get(&(dimension, key))
                    && let Some(cell) = key.cell(at)
                    && tile
                        .pixels
                        .get(cell * 4 + 3)
                        .is_some_and(|alpha| *alpha != 0)
                {
                    return Some(key);
                }
            }
            None
        }
        pub fn detailed_at(&self, dimension: Dimension, layer: Layer, at: [i32; 2]) -> bool {
            self.detailed
                .contains(&(dimension, layer, at[0].div_euclid(16), at[1].div_euclid(16)))
        }
        /// Detailed chunk count and LOD footprint count in a half-open block rectangle.
        pub fn counts(&self, dimension: Dimension, layer: Layer, rect: [i64; 4]) -> (usize, usize) {
            let detailed = self
                .detailed
                .iter()
                .filter(|(d, l, x, z)| {
                    *d == dimension
                        && *l == layer
                        && intersects(rect, [i64::from(*x) * 16, i64::from(*z) * 16, 16, 16])
                })
                .count();
            let lod = if layer == Layer::Surface {
                self.lod
                    .iter()
                    .filter(|(d, k, m)| *d == dimension && k.intersects_known(*m, rect))
                    .count()
            } else {
                0
            };
            (detailed, lod)
        }
    }
    pub fn intersects(a: [i64; 4], b: [i64; 4]) -> bool {
        a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
    }
    /// Floor division is essential at negative region/chunk boundaries.
    pub fn chunk_region(block: [i32; 2]) -> ([i32; 2], [i32; 2]) {
        let chunk = block.map(|v| v.div_euclid(16));
        (chunk, chunk.map(|v| v.div_euclid(32)))
    }
    #[derive(Clone)]
    pub struct LodDraw {
        pub key: LodKey,
        pub uv_origin: [usize; 2],
        pub fragments: Vec<[i64; 4]>,
    }
    /// Cut out *whole* detailed chunks, even known void/transparent pixels. LOD
    /// must never shine through them. Subdivide only near detail, not into all chunks.
    fn lod_fragments(key: LodKey, detailed: &BTreeSet<[i32; 2]>) -> Vec<[i64; 4]> {
        let mut stack = vec![key.bounds()];
        let mut out = Vec::new();
        while let Some(rect) = stack.pop() {
            let overlap = detailed
                .range(
                    [
                        ((rect[0] / 16).clamp(i64::from(i32::MIN), i64::from(i32::MAX))) as i32,
                        i32::MIN,
                    ]
                        ..=[
                            (((rect[0] + rect[2] - 1) / 16)
                                .clamp(i64::from(i32::MIN), i64::from(i32::MAX)))
                                as i32,
                            i32::MAX,
                        ],
                )
                .any(|[x, z]| intersects(rect, [i64::from(*x) * 16, i64::from(*z) * 16, 16, 16]));
            if !overlap {
                out.push(rect);
            } else if rect[2] > 16 {
                let half = rect[2] / 2;
                for z in [rect[1], rect[1] + half] {
                    for x in [rect[0], rect[0] + half] {
                        stack.push([x, z, half, half]);
                    }
                }
            }
        }
        out
    }

    /// Block-resolution terrain, uploaded only when the repository changes.
    pub struct TerrainAtlas {
        pub serial: u64,
        pub pixels: Vec<u8>,
        pub height: usize,
        pub chunks: Vec<[i32; 2]>,
        pub coarse: Vec<LodDraw>,
        pub region: [i64; 4],
        pub width: usize,
        pub lod: usize,
    }
    impl TerrainAtlas {
        fn levels(self) -> Vec<Arc<Self>> {
            // Keep original LOD texels at every zoom. Only detailed chunk
            // textures use the optional distant smoothing levels.
            if !self.coarse.is_empty() && self.chunks.is_empty() {
                let atlas = Arc::new(self);
                return vec![atlas; 5];
            }
            if !self.coarse.is_empty() {
                let mut levels = vec![Arc::new(self)];
                for lod in 1..=4 {
                    let source = &levels[0];
                    let step = 1usize << lod;
                    let size = 16 / step;
                    let columns = 1024 / size;
                    let fine_height = source.chunks.len().div_ceil(columns) * size;
                    let height = (fine_height + source.coarse.len().div_ceil(64) * 16).max(16);
                    let mut pixels = vec![0; 1024 * height * 4];
                    for slot in 0..source.chunks.len() {
                        for y in 0..size {
                            for x in 0..size {
                                let mut rgba = [0u32; 4];
                                for dy in 0..step {
                                    for dx in 0..step {
                                        let index = ((slot / 64 * 16 + y * step + dy) * 1024
                                            + slot % 64 * 16
                                            + x * step
                                            + dx)
                                            * 4;
                                        let a = u32::from(source.pixels[index + 3]);
                                        for (c, sum) in rgba[..3].iter_mut().enumerate() {
                                            *sum += u32::from(source.pixels[index + c]) * a;
                                        }
                                        rgba[3] += a;
                                    }
                                }
                                let index = ((slot / columns * size + y) * 1024
                                    + slot % columns * size
                                    + x)
                                    * 4;
                                for c in 0..3 {
                                    pixels[index + c] =
                                        ((rgba[c] + rgba[3] / 2) / rgba[3].max(1)) as u8;
                                }
                                pixels[index + 3] = ((rgba[3] + (step * step / 2) as u32)
                                    / (step * step) as u32)
                                    as u8;
                            }
                        }
                    }
                    let mut coarse = source.coarse.clone();
                    for (i, draw) in coarse.iter_mut().enumerate() {
                        let origin = [i % 64 * 16, fine_height + i / 64 * 16];
                        for row in 0..16 {
                            let from = ((draw.uv_origin[1] + row) * 1024 + draw.uv_origin[0]) * 4;
                            let to = ((origin[1] + row) * 1024 + origin[0]) * 4;
                            pixels[to..to + 64].copy_from_slice(&source.pixels[from..from + 64]);
                        }
                        draw.uv_origin = origin;
                    }
                    levels.push(Arc::new(Self {
                        serial: source.serial,
                        pixels,
                        width: 1024,
                        height,
                        chunks: source.chunks.clone(),
                        coarse,
                        lod,
                        region: source.region,
                    }));
                }
                return levels;
            }
            let mut levels = vec![Arc::new(self)];
            for lod in 1..=4 {
                let source = &levels[lod - 1];
                let width = source.width / 2;
                let height = source.height / 2;
                let mut pixels = vec![0; width * height * 4];
                for y in 0..height {
                    for x in 0..width {
                        let mut rgba = [0u32; 4];
                        for dy in 0..2 {
                            for dx in 0..2 {
                                let i = ((y * 2 + dy) * source.width + x * 2 + dx) * 4;
                                let alpha = u32::from(source.pixels[i + 3]);
                                for (channel, sum) in rgba[..3].iter_mut().enumerate() {
                                    *sum += u32::from(source.pixels[i + channel]) * alpha;
                                }
                                rgba[3] += alpha;
                            }
                        }
                        let i = (y * width + x) * 4;
                        for channel in 0..3 {
                            pixels[i + channel] =
                                ((rgba[channel] + rgba[3] / 2) / rgba[3].max(1)) as u8;
                        }
                        pixels[i + 3] = ((rgba[3] + 2) / 4) as u8;
                    }
                }
                levels.push(Arc::new(Self {
                    serial: source.serial,
                    pixels,
                    width,
                    height,
                    chunks: source.chunks.clone(),
                    coarse: source.coarse.clone(),
                    region: source.region,
                    lod,
                }));
            }
            levels
        }

        pub fn covers(&self, view: &View) -> bool {
            (0..2).all(|axis| {
                view.center[axis] - view.span[axis] / 2.0 >= self.region[axis] as f64
                    && view.center[axis] + view.span[axis] / 2.0
                        <= (self.region[axis] + self.region[axis + 2]) as f64
            })
        }
        pub fn coarse_quad(
            &self,
            view: &View,
            rect: [f32; 4],
            draw: &LodDraw,
            part: [i64; 4],
        ) -> Option<([f32; 4], [f32; 4])> {
            let bounds = draw.key.bounds();
            let mut src = [0.0; 4];
            let mut dst = [0.0; 4];
            for axis in 0..2 {
                let low = view.center[axis] - view.span[axis] / 2.0;
                let start = (part[axis] as f64).max(low);
                let end = ((part[axis] + part[axis + 2]) as f64).min(low + view.span[axis]);
                if end <= start {
                    return None;
                }
                let origin = draw.uv_origin[axis];
                src[axis] = origin as f32
                    + ((start - bounds[axis] as f64) * 16.0 / bounds[2] as f64) as f32;
                src[axis + 2] = ((end - start) * 16.0 / bounds[2] as f64) as f32;
                dst[axis] = rect[axis] + ((start - low) / view.span[axis]) as f32 * rect[axis + 2];
                dst[axis + 2] = ((end - start) / view.span[axis]) as f32 * rect[axis + 2];
            }
            Some((src, dst))
        }

        /// Clip in world space before converting to GUI coordinates. UVs stay
        /// fractional so GPU nearest sampling preserves block boundaries.
        pub fn quad(
            &self,
            view: &View,
            rect: [f32; 4],
            chunk: [i32; 2],
            slot: usize,
        ) -> Option<([f32; 4], [f32; 4])> {
            let mut src = [0.0; 4];
            let mut dst = [0.0; 4];
            for axis in 0..2 {
                let origin = f64::from(chunk[axis]) * 16.0;
                let low = view.center[axis] - view.span[axis] / 2.0;
                let start = origin.max(low);
                let end = (origin + 16.0).min(low + view.span[axis]);
                if end <= start {
                    return None;
                }
                let size = 16usize >> self.lod;
                let columns = self.width / size;
                let atlas_origin = if axis == 0 {
                    slot % columns * size
                } else {
                    slot / columns * size
                };
                src[axis] = atlas_origin as f32 + ((start - origin) / 16.0 * size as f64) as f32;
                src[axis + 2] = ((end - start) / 16.0 * size as f64) as f32;
                dst[axis] = rect[axis] + ((start - low) / view.span[axis]) as f32 * rect[axis + 2];
                dst[axis + 2] = ((end - start) / view.span[axis]) as f32 * rect[axis + 2];
            }
            Some((src, dst))
        }
    }
    struct AtlasCache {
        key: (u64, Dimension, Layer, u64),
        levels: Vec<Arc<TerrainAtlas>>,
    }
    pub struct Frame {
        pub serial: u64,
        pub view: View,
        pub pixels: Vec<u8>,
        pub known: usize,
        pub atlas: Option<Arc<TerrainAtlas>>,
    }
    type Key = (Dimension, Layer, i32, i32);
    #[derive(Default)]
    pub struct Cache {
        tiles: BTreeMap<Key, (u64, Tile)>,
        lod_tiles: BTreeMap<(Dimension, LodKey), (u64, Arc<Tile>)>,
        clock: u64,
    }
    impl Cache {
        pub fn insert(&mut self, dimension: Dimension, layer: Layer, chunk: [i32; 2], tile: Tile) {
            self.clock += 1;
            self.tiles
                .insert((dimension, layer, chunk[0], chunk[1]), (self.clock, tile));
        }
        pub fn insert_lod(&mut self, dimension: Dimension, key: LodKey, tile: Tile) -> bool {
            if key.level > 22
                || tile.pixels.len() != 1024
                || !tile.pixels.chunks_exact(4).any(|p| p[3] != 0)
            {
                return false;
            }
            if let Some((stamp, old)) = self.lod_tiles.get_mut(&(dimension, key)) {
                let old = Arc::make_mut(old);
                let mut changed = false;
                for i in 0..256 {
                    if old.pixels[i * 4 + 3] == 0 && tile.pixels[i * 4 + 3] != 0 {
                        old.pixels[i * 4..i * 4 + 4]
                            .copy_from_slice(&tile.pixels[i * 4..i * 4 + 4]);
                        old.heights[i] = tile.heights[i];
                        if !tile.biomes.is_empty() {
                            if old.biomes.len() != 256 {
                                old.biomes.resize(256, None);
                            }
                            old.biomes[i] = tile.biomes.get(i).cloned().flatten();
                        }
                        changed = true;
                    }
                }
                if !tile.biomes.is_empty() {
                    if old.biomes.len() != 256 {
                        old.biomes.resize(256, None);
                    }
                    for i in 0..256 {
                        if old.biomes[i].is_none()
                            && let Some(name) = tile.biomes.get(i).cloned().flatten()
                        {
                            old.biomes[i] = Some(name);
                            changed = true;
                        }
                    }
                }
                if changed {
                    self.clock += 1;
                    *stamp = self.clock;
                }
                return changed;
            }
            self.clock += 1;
            self.lod_tiles
                .insert((dimension, key), (self.clock, Arc::new(tile)));
            true
        }
        pub fn coverage(&self) -> Coverage {
            Coverage {
                session: 0,
                detailed: self.tiles.keys().copied().collect(),
                lod_data: self
                    .lod_tiles
                    .iter()
                    .map(|(key, (_, tile))| (*key, tile.clone()))
                    .collect(),
                lod: self
                    .lod_tiles
                    .iter()
                    .map(|((d, key), (_, tile))| {
                        let mut mask = [0; 4];
                        for (i, pixel) in tile.pixels.chunks_exact(4).enumerate() {
                            if pixel[3] != 0 {
                                mask[i / 64] |= 1 << (i % 64);
                            }
                        }
                        (*d, *key, mask)
                    })
                    .collect(),
            }
        }
        fn lod_pixel(&self, dimension: Dimension, layer: Layer, block: [i32; 2]) -> Option<&[u8]> {
            if layer != Layer::Surface || self.lod_tiles.is_empty() {
                return None;
            }
            for level in 0..=22 {
                let size = 16_i64 << level;
                let key = LodKey {
                    x: (i64::from(block[0]).div_euclid(size)) as i32,
                    z: (i64::from(block[1]).div_euclid(size)) as i32,
                    level,
                };
                if let Some((_, tile)) = self.lod_tiles.get(&(dimension, key)) {
                    let cell = key.cell(block)?;
                    let pixel = tile.pixels.get(cell * 4..cell * 4 + 4)?;
                    if pixel[3] != 0 {
                        return Some(pixel);
                    }
                }
            }
            None
        }
        fn gpu_atlas_fits(&self, view: &View) -> bool {
            let region = view.atlas_region();
            let fine = self
                .tiles
                .keys()
                .filter(|key| {
                    key.0 == view.dimension
                        && key.1 == view.layer
                        && intersects(
                            region,
                            [i64::from(key.2) * 16, i64::from(key.3) * 16, 16, 16],
                        )
                })
                .count();
            let coarse = if view.layer == Layer::Surface {
                self.lod_tiles
                    .keys()
                    .filter(|(d, key)| *d == view.dimension && intersects(region, key.bounds()))
                    .count()
            } else {
                0
            };
            fine.div_ceil(64) + coarse.div_ceil(64) <= 64
        }
        fn terrain_atlas(&self, view: &View, serial: u64) -> TerrainAtlas {
            let region = view.atlas_region();
            let chunks: Vec<_> = self
                .tiles
                .keys()
                .filter(|key| {
                    key.0 == view.dimension
                        && key.1 == view.layer
                        && intersects(
                            region,
                            [i64::from(key.2) * 16, i64::from(key.3) * 16, 16, 16],
                        )
                })
                .map(|key| [key.2, key.3])
                .collect();
            let detailed: BTreeSet<_> = chunks.iter().copied().collect();
            let mut coarse: Vec<_> = if view.layer == Layer::Surface {
                self.lod_tiles
                    .keys()
                    .filter(|(d, key)| *d == view.dimension && intersects(region, key.bounds()))
                    .map(|(_, key)| *key)
                    .collect()
            } else {
                Vec::new()
            };
            coarse.sort_by_key(|key| std::cmp::Reverse(key.level));
            let coarse: Vec<LodDraw> = coarse
                .into_iter()
                .enumerate()
                .map(|(i, key)| LodDraw {
                    key,
                    uv_origin: [i % 64 * 16, chunks.len().div_ceil(64) * 16 + i / 64 * 16],
                    fragments: lod_fragments(key, &detailed),
                })
                .collect();
            let height = (chunks.len().div_ceil(64) + coarse.len().div_ceil(64)).max(1) * 16;
            let mut pixels = vec![0; 1024 * height * 4];
            for (slot, chunk) in chunks.iter().enumerate() {
                // Bake lighting once per block, including neighbouring chunks.
                // No screen-space raster is produced for the large map.
                let tile = self.render(
                    View {
                        center: [
                            f64::from(chunk[0]) * 16.0 + 8.0,
                            f64::from(chunk[1]) * 16.0 + 8.0,
                        ],
                        span: [16.0; 2],
                        width: 16,
                        height: 16,
                        angle: 0.0,
                        round: false,
                        ..view.clone()
                    },
                    serial,
                );
                for row in 0..16 {
                    let target = ((slot / 64 * 16 + row) * 1024 + slot % 64 * 16) * 4;
                    pixels[target..target + 64]
                        .copy_from_slice(&tile.pixels[row * 64..row * 64 + 64]);
                }
            }
            for draw in &coarse {
                let tile = &self.lod_tiles[&(view.dimension, draw.key)].1;
                for y in 0..16 {
                    let src = y * 64;
                    let dst = ((draw.uv_origin[1] + y) * 1024 + draw.uv_origin[0]) * 4;
                    pixels[dst..dst + 64].copy_from_slice(&tile.pixels[src..src + 64]);
                }
            }
            TerrainAtlas {
                serial,
                pixels,
                height,
                chunks,
                coarse,
                region,
                width: 1024,
                lod: 0,
            }
        }
        pub fn render(&self, mut view: View, serial: u64) -> Frame {
            let scale = (view.width.max(view.height) as f64 / 1024.0).max(1.0);
            view.width = ((view.width as f64 / scale).ceil() as usize).clamp(1, 1024);
            view.height = ((view.height as f64 / scale).ceil() as usize).clamp(1, 1024);
            let width = view.width;
            let height = view.height;
            let mut pixels = vec![0; width * height * 4];
            let mut known = 0;
            let (sin, cos) = view.angle.sin_cos();
            let step_x = view.span[0] / width as f64;
            let step_z = view.span[1] / height as f64;
            let (half_x, half_z) = (width as f64 / 2.0, height as f64 / 2.0);
            // The tile a pixel reads changes once per 16-block cell, not per
            // pixel: look it up again only when the chunk changes. The north
            // and west neighbours are separate chunks only at a chunk's edge.
            let mut tile: Option<((i32, i32), Option<&Tile>)> = None;
            let mut north: Option<((i32, i32), Option<&Tile>)> = None;
            let mut west: Option<((i32, i32), Option<&Tile>)> = None;
            for y in 0..height {
                let dz = (y as f64 + 0.5 - half_z) * step_z;
                let (tilt_x, tilt_z) = (dz * sin, dz * cos);
                for x in 0..width {
                    if view.round {
                        let nx = (x as f64 + 0.5) / width as f64 - 0.5;
                        let ny = (y as f64 + 0.5) / height as f64 - 0.5;
                        if nx * nx + ny * ny > 0.25 {
                            continue;
                        }
                    }
                    let dx = (x as f64 + 0.5 - half_x) * step_x;
                    let bx = (view.center[0] + dx * cos - tilt_x).floor() as i32;
                    let bz = (view.center[1] + dx * sin + tilt_z).floor() as i32;
                    let chunk = (bx.div_euclid(16), bz.div_euclid(16));
                    if tile.is_none_or(|(key, _)| key != chunk) {
                        tile = Some((
                            chunk,
                            self.tiles
                                .get(&(view.dimension, view.layer, chunk.0, chunk.1))
                                .map(|(_, tile)| tile),
                        ));
                    }
                    let current = tile.and_then(|(_, tile)| tile);
                    let i = (y * width + x) * 4;
                    let local = (bz.rem_euclid(16) * 16 + bx.rem_euclid(16)) as usize;
                    let Some(current) = current else {
                        if let Some(color) = self.lod_pixel(view.dimension, view.layer, [bx, bz]) {
                            pixels[i..i + 4].copy_from_slice(color);
                            known += 1;
                        }
                        continue;
                    };
                    let Some(color) = current.pixels.get(local * 4..local * 4 + 4) else {
                        continue;
                    };
                    // Relief shading reads the column north and west of this
                    // one from their own tiles, so chunk borders have no seam
                    // and slopes stay readable at every zoom.
                    let north_z = bz - 1;
                    let north_chunk = (bx.div_euclid(16), north_z.div_euclid(16));
                    let neighbour = if north_chunk == chunk {
                        Some(current)
                    } else {
                        if north.is_none_or(|(key, _)| key != north_chunk) {
                            north = Some((
                                north_chunk,
                                self.tiles
                                    .get(&(
                                        view.dimension,
                                        view.layer,
                                        north_chunk.0,
                                        north_chunk.1,
                                    ))
                                    .map(|(_, tile)| tile),
                            ));
                        }
                        north.and_then(|(_, tile)| tile)
                    };
                    let west_x = bx - 1;
                    let west_chunk = (west_x.div_euclid(16), bz.div_euclid(16));
                    let west_tile = if west_chunk == chunk {
                        Some(current)
                    } else {
                        if west.is_none_or(|(key, _)| key != west_chunk) {
                            west = Some((
                                west_chunk,
                                self.tiles
                                    .get(&(view.dimension, view.layer, west_chunk.0, west_chunk.1))
                                    .map(|(_, tile)| tile),
                            ));
                        }
                        west.and_then(|(_, tile)| tile)
                    };
                    let height = current.heights[local];
                    let north_height = neighbour.map_or(height, |tile| {
                        let index = (north_z.rem_euclid(16) * 16 + bx.rem_euclid(16)) as usize;
                        tile.heights[index]
                    });
                    let west_height = west_tile.map_or(height, |tile| {
                        let index = (bz.rem_euclid(16) * 16 + west_x.rem_euclid(16)) as usize;
                        tile.heights[index]
                    });
                    // Light from the north-west: a rise towards it is darker,
                    // a slope away from it is brighter. The base keeps the map
                    // readable instead of the old flat 0.8 step.
                    let relief = f32::from((north_height - height).clamp(-64, 64) as i8) * -0.085
                        + f32::from((west_height - height).clamp(-64, 64) as i8) * -0.05;
                    let shade = if current.walls[local] {
                        0.55
                    } else {
                        (1.09 + relief).clamp(0.65, 1.38)
                    };
                    for channel in 0..3 {
                        pixels[i + channel] = (f32::from(color[channel]) * shade).round() as u8;
                    }
                    pixels[i + 3] = color[3];
                    known += 1;
                }
            }
            Frame {
                serial,
                view,
                pixels,
                known,
                atlas: None,
            }
        }
    }
    enum Job {
        Lod {
            session: u64,
            dimension: Dimension,
            key: LodKey,
            sample: Box<dyn FnOnce() -> Tile + Send>,
        },
        Chunk {
            session: u64,
            dimension: Dimension,
            layer: Layer,
            chunk: [i32; 2],
            sample: Box<dyn FnOnce() -> Tile + Send>,
        },
        Wake,
    }
    type PortalKey = (Dimension, i32, i32, Portal);
    type PortalMap = BTreeMap<PortalKey, [i32; 3]>;

    pub struct Service {
        tx: Mutex<Option<mpsc::SyncSender<Job>>>,
        worker: Mutex<Option<JoinHandle<()>>>,
        latest: Arc<Mutex<Option<View>>>,
        output: Arc<Mutex<Option<Arc<Frame>>>>,
        session: Arc<AtomicU64>,
        resident: Arc<Mutex<BTreeSet<Key>>>,
        coverage: Arc<Mutex<Arc<Coverage>>>,
        portals: Arc<Mutex<PortalMap>>,
    }
    impl Default for Service {
        fn default() -> Self {
            Self::new()
        }
    }
    impl Service {
        pub fn new() -> Self {
            let (tx, rx) = mpsc::sync_channel(8);
            let latest = Arc::new(Mutex::new(None::<View>));
            let output = Arc::new(Mutex::new(None));
            let session = Arc::new(AtomicU64::new(0));
            let resident = Arc::new(Mutex::new(BTreeSet::new()));
            let portals = Arc::new(Mutex::new(BTreeMap::new()));
            let coverage = Arc::new(Mutex::new(Arc::new(Coverage::default())));
            let coverage_state = coverage.clone();
            let residency = resident.clone();
            let portal_points = portals.clone();
            let (input, result, active) = (latest.clone(), output.clone(), session.clone());
            let worker = thread::Builder::new()
                .name("map-terrain".into())
                .spawn(move || {
                    let mut cache = Cache::default();
                    let mut current = 0;
                    let mut serial = 0;
                    let mut atlas: Option<AtlasCache> = None;
                    while let Ok(first) = rx.recv() {
                        let mut jobs = vec![first];
                        jobs.extend(rx.try_iter().take(7));
                        let mut changed = false;
                        if current != active.load(Ordering::Acquire) {
                            cache = Cache::default();
                            current = active.load(Ordering::Acquire);
                        }
                        for job in jobs {
                            let job = match job {
                                Job::Lod {
                                    session,
                                    dimension,
                                    key,
                                    sample,
                                } => {
                                    if session == active.load(Ordering::Acquire) {
                                        let tile = sample();
                                        if session == active.load(Ordering::Acquire) {
                                            changed |= cache.insert_lod(dimension, key, tile);
                                        }
                                    }
                                    continue;
                                }
                                other => other,
                            };
                            if let Job::Chunk {
                                session,
                                dimension,
                                layer,
                                chunk,
                                sample,
                            } = job
                            {
                                if session != active.load(Ordering::Acquire) {
                                    continue;
                                }
                                if current != session {
                                    cache = Cache::default();
                                    current = session;
                                }
                                let tile = sample();
                                if session == active.load(Ordering::Acquire) {
                                    let points = tile.portals.clone();
                                    cache.insert(dimension, layer, chunk, tile);
                                    if !points.is_empty() {
                                        let mut portals =
                                            portal_points.lock().unwrap_or_else(|e| e.into_inner());
                                        for (at, kind) in points {
                                            portals.insert((dimension, at[0], at[2], kind), at);
                                        }
                                    }
                                    changed = true;
                                }
                            }
                        }
                        if changed {
                            let mut entries = residency.lock().unwrap_or_else(|e| e.into_inner());
                            if current == active.load(Ordering::Acquire) {
                                *entries = cache.tiles.keys().copied().collect();
                                *coverage_state.lock().unwrap_or_else(|e| e.into_inner()) = {
                                    let mut summary = cache.coverage();
                                    summary.session = current;
                                    Arc::new(summary)
                                };
                            }
                        }
                        let Some(view) = input.lock().unwrap_or_else(|e| e.into_inner()).clone()
                        else {
                            continue;
                        };
                        if view.session != active.load(Ordering::Acquire) {
                            continue;
                        }
                        if current != view.session {
                            cache = Cache::default();
                            current = view.session;
                            changed = true;
                        }
                        let same = result
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .as_ref()
                            .is_some_and(|f: &Arc<Frame>| f.view == view);
                        if same && !changed {
                            continue;
                        }
                        serial += 1;
                        let frame = if view.large && cache.gpu_atlas_fits(&view) {
                            let key = (view.session, view.dimension, view.layer, cache.clock);
                            if atlas
                                .as_ref()
                                .is_none_or(|a| a.key != key || !a.levels[0].covers(&view))
                            {
                                atlas = Some(AtlasCache {
                                    key,
                                    levels: cache.terrain_atlas(&view, serial).levels(),
                                });
                            }
                            let terrain = atlas.as_ref().unwrap().levels[view.lod()].clone();
                            Arc::new(Frame {
                                serial,
                                view,
                                pixels: Vec::new(),
                                known: terrain.chunks.len() + terrain.coarse.len(),
                                atlas: Some(terrain),
                            })
                        } else {
                            Arc::new(cache.render(view, serial))
                        };
                        if frame.view.session == active.load(Ordering::Acquire) {
                            *result.lock().unwrap_or_else(|e| e.into_inner()) = Some(frame);
                        }
                    }
                })
                .map_err(|error| eprintln!("Map worker failed: {error}"))
                .ok();
            Self {
                tx: Mutex::new(Some(tx)),
                worker: Mutex::new(worker),
                latest,
                output,
                session,
                resident,
                coverage,
                portals,
            }
        }
        pub fn reset(&self, session: u64) {
            self.session.store(session, Ordering::Release);
            *self.coverage.lock().unwrap_or_else(|e| e.into_inner()) =
                Arc::new(Coverage::default());
            self.resident
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
            self.portals
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
            *self.output.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        pub fn coverage(&self) -> Arc<Coverage> {
            let coverage = self.coverage.lock().unwrap_or_else(|e| e.into_inner());
            if coverage.session == self.session.load(Ordering::Acquire) {
                coverage.clone()
            } else {
                Arc::new(Coverage::default())
            }
        }
        pub fn submit_lod(
            &self,
            session: u64,
            dimension: Dimension,
            key: LodKey,
            sample: impl FnOnce() -> Tile + Send + 'static,
        ) -> bool {
            self.tx
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .is_some_and(|tx| {
                    tx.try_send(Job::Lod {
                        session,
                        dimension,
                        key,
                        sample: Box::new(sample),
                    })
                    .is_ok()
                })
        }
        pub fn portals(&self) -> Vec<(Dimension, [i32; 3], Portal)> {
            self.portals
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .map(|((dimension, _, _, kind), at)| (*dimension, *at, *kind))
                .collect()
        }
        pub fn requested(&self) -> Option<View> {
            self.latest
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        }
        pub fn contains(&self, dimension: Dimension, layer: Layer, chunk: [i32; 2]) -> bool {
            self.resident
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains(&(dimension, layer, chunk[0], chunk[1]))
        }
        pub fn submit(
            &self,
            session: u64,
            dimension: Dimension,
            layer: Layer,
            chunk: [i32; 2],
            sample: impl FnOnce() -> Tile + Send + 'static,
        ) -> bool {
            self.tx
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .is_some_and(|tx| {
                    tx.try_send(Job::Chunk {
                        session,
                        dimension,
                        layer,
                        chunk,
                        sample: Box::new(sample),
                    })
                    .is_ok()
                })
        }
        pub fn request(&self, view: View) {
            let mut latest = self.latest.lock().unwrap_or_else(|e| e.into_inner());
            if latest.as_ref() == Some(&view) {
                return;
            }
            *latest = Some(view);
            drop(latest);
            if let Some(tx) = self.tx.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                let _ = tx.try_send(Job::Wake);
            }
        }
        pub fn frame(&self) -> Option<Arc<Frame>> {
            self.output
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        }
        pub fn shutdown(&self) {
            self.tx.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(worker) = self.worker.lock().unwrap_or_else(|e| e.into_inner()).take() {
                let _ = worker.join();
            }
        }
    }
    impl Drop for Service {
        fn drop(&mut self) {
            self.shutdown();
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        fn view(d: Dimension) -> View {
            View {
                session: 1,
                dimension: d,
                layer: Layer::Surface,
                center: [-8.0, 8.0],
                span: [16.0, 16.0],
                width: 16,
                height: 16,
                angle: 0.0,
                round: false,
                large: true,
                smooth: false,
            }
        }
        #[test]
        fn grass_blocks_take_the_biome_grass_colour() {
            assert_eq!(
                block_map_color("minecraft:grass_block", 0x7fb238, Some(0x91bd59)),
                0x91bd59
            );
            assert_eq!(
                block_map_color("minecraft:grass_block", 0x7fb238, None),
                0x7fb238
            );
            assert_eq!(
                block_map_color("minecraft:stone", 0x707070, Some(0x91bd59)),
                0x707070
            );
            assert_eq!(
                block_map_color("minecraft:grass", 0x7fb238, Some(0xffffff)),
                0xffffff
            );
        }
        fn terrain_fixture() -> Cache {
            let mut cache = Cache::default();
            for z in -2..=2 {
                for x in -2..=2 {
                    if [x, z] == [0, 0] {
                        continue;
                    }
                    cache.insert(
                        Dimension::Overworld,
                        Layer::Surface,
                        [x, z],
                        Tile {
                            pixels: (0..256)
                                .flat_map(|i| [i as u8, 80, 120, if i % 7 == 0 { 0 } else { 255 }])
                                .collect(),
                            heights: std::array::from_fn(|i| (i % 19) as i32 + x + z),
                            walls: std::array::from_fn(|i| i % 11 == 0),
                            ..Tile::default()
                        },
                    );
                }
            }
            cache
        }
        #[test]
        fn lod_averages_alpha_without_bleeding_between_slots() {
            let mut cache = Cache::default();
            for (chunk, color) in [([0, 0], [240, 0, 0, 255]), ([1, 0], [0, 0, 240, 255])] {
                let mut tile = Tile {
                    pixels: color.repeat(256),
                    ..Tile::default()
                };
                if chunk == [0, 0] {
                    tile.pixels[0..4].copy_from_slice(&[0, 255, 0, 0]);
                }
                cache.insert(Dimension::Overworld, Layer::Surface, chunk, tile);
            }
            let mut view = view(Dimension::Overworld);
            let levels = cache.terrain_atlas(&view, 7).levels();
            assert_eq!(levels.len(), 5);
            for (lod, atlas) in levels.iter().enumerate().skip(1) {
                let size = 16 >> lod;
                assert_eq!(atlas.width, 64 * size);
                assert_eq!(atlas.pixels[0], 255);
                assert_eq!(atlas.pixels[1], 0); // transparent green does not tint red
                assert_eq!(&atlas.pixels[size * 4..size * 4 + 4], &[0, 0, 255, 255]);
                let (uv, _) = atlas
                    .quad(&view, [0.0, 0.0, 16.0, 16.0], [-1, 0], 1)
                    .unwrap();
                assert_eq!(uv, [size as f32, 0.0, size as f32, size as f32]);
            }
            assert_eq!(levels[1].pixels[3], 191);
            view.width = 100;
            view.height = 100;
            view.smooth = true;
            for (span, expected) in [(100.0, 0), (200.0, 1), (800.0, 3), (6400.0, 4)] {
                view.span = [span; 2];
                assert_eq!(view.lod(), expected);
            }
            view.smooth = false;
            assert_eq!(view.lod(), 0);
        }
        #[test]
        fn gpu_atlas_matches_cpu_pixels_and_clips_fractional_edges() {
            let cache = terrain_fixture();
            let mut view = view(Dimension::Overworld);
            view.large = true;
            view.width = 123;
            view.height = 91;
            view.center = [-3.37, 7.19];
            let atlas = cache.terrain_atlas(&view, 1);
            assert!(atlas.height <= 1024);
            for span in [3.1, 27.7, 137.3] {
                view.span = [span, span * 0.73];
                let reference = cache.render(view.clone(), 2);
                let mut gpu = vec![0; reference.pixels.len()];
                let rect = [0.0, 0.0, view.width as f32, view.height as f32];
                for (slot, chunk) in atlas.chunks.iter().enumerate() {
                    let Some((src, dst)) = atlas.quad(&view, rect, *chunk, slot) else {
                        continue;
                    };
                    assert!(dst[0] >= 0.0 && dst[1] >= 0.0);
                    assert!(dst[0] + dst[2] <= rect[2] + 0.001);
                    assert!(dst[1] + dst[3] <= rect[3] + 0.001);
                    for y in 0..view.height {
                        for x in 0..view.width {
                            let px = x as f32 + 0.5;
                            let py = y as f32 + 0.5;
                            if px < dst[0]
                                || px >= dst[0] + dst[2]
                                || py < dst[1]
                                || py >= dst[1] + dst[3]
                            {
                                continue;
                            }
                            let u = (src[0] + (px - dst[0]) / dst[2] * src[2]).floor() as usize;
                            let v = (src[1] + (py - dst[1]) / dst[3] * src[3]).floor() as usize;
                            let from = (v * 1024 + u) * 4;
                            let to = (y * view.width + x) * 4;
                            gpu[to..to + 4].copy_from_slice(&atlas.pixels[from..from + 4]);
                        }
                    }
                }
                assert_eq!(gpu, reference.pixels, "span {span}");
            }
        }
        #[test]
        #[ignore = "manual release CPU timing; does not measure GPU execution or game FPS"]
        fn benchmark_worldmap_cpu_work() {
            use std::{hint::black_box, time::Instant};
            let mut cache = Cache::default();
            for z in -32..32 {
                for x in -32..32 {
                    cache.insert(
                        Dimension::Overworld,
                        Layer::Surface,
                        [x, z],
                        Tile {
                            pixels: [80, 120, 40, 255].repeat(256),
                            ..Tile::default()
                        },
                    );
                }
            }
            let mut view = view(Dimension::Overworld);
            view.width = 1024;
            view.height = 1024;
            view.large = true;
            let start = Instant::now();
            let atlas = cache.terrain_atlas(&view, 1);
            eprintln!(
                "atlas preparation {:?}, {} bytes",
                start.elapsed(),
                atlas.pixels.len()
            );
            for span in [32.0, 256.0, 2048.0] {
                view.span = [span; 2];
                let start = Instant::now();
                for i in 0..60 {
                    view.center[0] = i as f64 * 0.137;
                    black_box(cache.render(view.clone(), i));
                }
                let old = start.elapsed().as_secs_f64() * 1000.0 / 60.0;
                let start = Instant::now();
                for i in 0..60 {
                    view.center[0] = i as f64 * 0.137;
                    for (slot, chunk) in atlas.chunks.iter().enumerate() {
                        black_box(atlas.quad(
                            black_box(&view),
                            [0.0, 0.0, 1024.0, 1024.0],
                            *chunk,
                            slot,
                        ));
                    }
                }
                eprintln!(
                    "span={span}: CPU raster {old:.3} ms/frame; GPU quad preparation {:.3} ms/frame",
                    start.elapsed().as_secs_f64() * 1000.0 / 60.0
                );
            }
        }
        #[test]
        fn a_stationary_view_is_not_evicted_by_background_terrain() {
            let mut cache = Cache::default();
            cache.insert(
                Dimension::Overworld,
                Layer::Surface,
                [-1, 0],
                Tile {
                    pixels: [10, 20, 30, 255].repeat(256),
                    ..Tile::default()
                },
            );
            for x in 1000..6000 {
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
            assert_eq!(cache.render(view(Dimension::Overworld), 1).known, 256);
            assert!(!view(Dimension::Overworld).demands(
                Dimension::Overworld,
                Layer::Surface,
                [10, 0]
            ));
        }
        #[test]
        fn dimensions_layers_and_negative_chunks_are_isolated() {
            let mut cache = Cache::default();
            cache.insert(
                Dimension::Overworld,
                Layer::Surface,
                [-1, 0],
                Tile {
                    pixels: [10, 20, 30, 255].repeat(256),
                    ..Tile::default()
                },
            );
            assert_eq!(cache.render(view(Dimension::Overworld), 1).known, 256);
            assert_eq!(cache.render(view(Dimension::Nether), 2).known, 0);
            let mut below = view(Dimension::Overworld);
            below.layer = Layer::Below(40);
            assert_eq!(cache.render(below, 3).known, 0);
        }
        #[test]
        fn eased_zoom_keeps_the_world_point_under_the_cursor() {
            let mut camera = Camera::new([12.0, -8.0], 4.0);
            let offset = [-50.0, 75.0];
            let anchor = [
                camera.center[0] + offset[0] * camera.zoom,
                camera.center[1] + offset[1] * camera.zoom,
            ];
            camera.zoom_at(1.0, offset);
            for _ in 0..120 {
                camera.advance(1.0 / 60.0);
            }
            assert_eq!(camera.zoom, 2.0);
            for axis in 0..2 {
                let under = camera.center[axis] + offset[axis] * camera.zoom;
                assert!((under - anchor[axis]).abs() < 1e-9, "axis {axis}");
            }
            assert!(!camera.moving());
        }
        #[test]
        fn camera_eases_instead_of_jumping_and_snaps_on_a_teleport() {
            let mut camera = Camera::new([0.0; 2], 2.0);
            camera.follow([32.0, 0.0]);
            camera.advance(1.0 / 60.0);
            assert!(camera.center[0] > 0.0 && camera.center[0] < 32.0);
            for _ in 0..240 {
                camera.advance(1.0 / 60.0);
            }
            assert!((camera.center[0] - 32.0).abs() < 0.01);
            camera.follow([1000.0, 0.0]);
            assert_eq!(camera.center, [1000.0, 0.0]);
            assert!(!camera.moving());
        }
        #[test]
        fn dragging_shifts_the_target_by_one_world_pixel_per_gui_pixel() {
            let mut camera = Camera::new([100.0, 0.0], 2.0);
            camera.pan([10.0, -5.0]);
            assert_eq!(camera.target, [80.0, 10.0]);
            for _ in 0..240 {
                camera.advance(1.0 / 60.0);
            }
            assert_eq!(camera.center, [80.0, 10.0]);
        }
        #[test]
        fn player_heading_tracks_yaw_and_rotating_maps() {
            let mut frame = view(Dimension::Overworld);
            for (yaw, expected) in [
                (0.0, [0.0, 1.0]),
                (90.0, [-1.0, 0.0]),
                (180.0, [0.0, -1.0]),
                (270.0, [1.0, 0.0]),
            ] {
                let heading = frame.player_heading(yaw, [128.0, 128.0]);
                assert!((heading[0] - expected[0]).abs() < 1e-6);
                assert!((heading[1] - expected[1]).abs() < 1e-6);
                frame.angle = (yaw + 180.0).to_radians();
                let heading = frame.player_heading(yaw, [128.0, 128.0]);
                assert!(heading[0].abs() < 1e-6);
                assert!((heading[1] + 1.0).abs() < 1e-6);
                frame.angle = 0.0;
            }
            frame.span = [200.0, 100.0];
            let heading = frame.player_heading(-45.0, [400.0, 100.0]);
            assert!((heading[0] / heading[1] - 2.0).abs() < 1e-6);
        }

        fn coarse_tile(rgb: [u8; 3]) -> Tile {
            Tile {
                pixels: rgb.into_iter().chain([255]).cycle().take(1024).collect(),
                ..Tile::default()
            }
        }
        #[test]
        fn real_lod_coverage_does_not_invent_chunks_or_visit_history() {
            let mut cache = Cache::default();
            let key = LodKey {
                x: -1,
                z: 0,
                level: 2,
            };
            assert!(cache.insert_lod(Dimension::Overworld, key, coarse_tile([80, 120, 40])));
            let mut v = view(Dimension::Overworld);
            v.center = [-8.0, 8.0];
            v.span = [16.0; 2];
            let frame = cache.render(v.clone(), 1);
            assert!(
                frame
                    .pixels
                    .chunks_exact(4)
                    .all(|p| p == [80, 120, 40, 255])
            );
            let coverage = cache.coverage();
            assert!(coverage.detailed.is_empty());
            assert_eq!(
                coverage.lod_at(Dimension::Overworld, Layer::Surface, [-1, 1]),
                Some(key)
            );
            assert_eq!(
                coverage.lod_at(Dimension::Nether, Layer::Surface, [-1, 1]),
                None
            );
            v.layer = Layer::Below(40);
            assert_eq!(cache.render(v, 2).known, 0);
            assert_eq!(chunk_region([-1, -513]), ([-1, -33], [-1, -2]));
        }
        #[test]
        fn detailed_void_always_masks_lod_in_cpu_and_gpu_maps() {
            let mut cache = Cache::default();
            let key = LodKey {
                x: -1,
                z: 0,
                level: 2,
            };
            cache.insert_lod(Dimension::Overworld, key, coarse_tile([90, 20, 10]));
            cache.insert(
                Dimension::Overworld,
                Layer::Surface,
                [-1, 0],
                Tile {
                    pixels: vec![0; 1024],
                    ..Tile::default()
                },
            );
            let mut v = view(Dimension::Overworld);
            v.center = [-8.0, 8.0];
            v.span = [16.0; 2];
            assert!(cache.render(v.clone(), 1).pixels.iter().all(|v| *v == 0));
            let atlas = cache.terrain_atlas(&v, 1);
            assert!(
                atlas.coarse[0]
                    .fragments
                    .iter()
                    .all(|part| !intersects(*part, [-16, 0, 16, 16]))
            );
            assert!(!cache.insert_lod(Dimension::Overworld, key, coarse_tile([255, 255, 255])));
            assert!(
                cache
                    .coverage()
                    .detailed_at(Dimension::Overworld, Layer::Surface, [-1, 1])
            );
        }
        #[test]
        fn lod_priority_precision_and_large_footprints_remain_bounded() {
            let mut cache = Cache::default();
            let coarse = LodKey {
                x: 0,
                z: 0,
                level: 22,
            };
            let finer = LodKey {
                x: 0,
                z: 0,
                level: 1,
            };
            cache.insert_lod(Dimension::Overworld, coarse, coarse_tile([10, 10, 10]));
            cache.insert_lod(Dimension::Overworld, finer, coarse_tile([20, 20, 20]));
            assert_eq!(
                cache
                    .coverage()
                    .lod_at(Dimension::Overworld, Layer::Surface, [5, 5]),
                Some(finer)
            );
            assert_eq!(
                cache.lod_pixel(Dimension::Overworld, Layer::Surface, [5, 5]),
                Some([20, 20, 20, 255].as_slice())
            );
            let detailed = BTreeSet::from([[0, 0]]);
            let fragments = lod_fragments(coarse, &detailed);
            assert!(fragments.len() < 100);
            assert!(
                fragments
                    .iter()
                    .all(|part| !intersects(*part, [0, 0, 16, 16]))
            );
            let mut partial = coarse_tile([1, 2, 3]);
            partial.pixels[4..].fill(0);
            let mut c = Cache::default();
            c.insert_lod(
                Dimension::Overworld,
                LodKey {
                    x: 0,
                    z: 0,
                    level: 6,
                },
                partial,
            );
            assert_eq!(
                c.coverage()
                    .counts(Dimension::Overworld, Layer::Surface, [512, 512, 512, 512]),
                (0, 0)
            );
        }
        #[test]
        fn detailed_arrivals_and_lod_beyond_old_limits_keep_all_known_terrain() {
            let mut cache = Cache::default();
            for x in 0..520 {
                cache.insert_lod(
                    Dimension::Overworld,
                    LodKey { x, z: 0, level: 1 },
                    coarse_tile([1, 2, 3]),
                );
            }
            assert_eq!(cache.lod_tiles.len(), 520);
            for x in 0..4096 {
                cache.insert(
                    Dimension::Overworld,
                    Layer::Surface,
                    [x, 0],
                    Tile::default(),
                );
            }
            assert_eq!(cache.tiles.len(), 4096);
            assert_eq!(cache.lod_tiles.len(), 520);
            assert!(cache.insert_lod(
                Dimension::Overworld,
                LodKey {
                    x: 521,
                    z: 0,
                    level: 1
                },
                coarse_tile([1, 2, 3])
            ));
        }
        #[test]
        fn newer_lod_revisions_fill_holes_without_overwriting_known_samples() {
            let mut cache = Cache::default();
            let key = LodKey {
                x: 0,
                z: 0,
                level: 1,
            };
            let mut partial = coarse_tile([10, 20, 30]);
            partial.pixels[4..].fill(0);
            assert!(cache.insert_lod(Dimension::Overworld, key, partial));
            assert!(cache.insert_lod(Dimension::Overworld, key, coarse_tile([40, 50, 60])));
            assert_eq!(
                cache.lod_pixel(Dimension::Overworld, Layer::Surface, [0, 0]),
                Some([10, 20, 30, 255].as_slice())
            );
            assert_eq!(
                cache.lod_pixel(Dimension::Overworld, Layer::Surface, [4, 0]),
                Some([40, 50, 60, 255].as_slice())
            );
            assert!(!cache.insert_lod(Dimension::Overworld, key, coarse_tile([99, 99, 99])));
        }

        #[test]
        fn old_lod_jobs_cannot_publish_coverage_into_a_new_world() {
            let service = Service::new();
            service.reset(1);
            let (started_tx, started_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            assert!(service.submit_lod(
                1,
                Dimension::Overworld,
                LodKey {
                    x: 0,
                    z: 0,
                    level: 1
                },
                move || {
                    started_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    coarse_tile([1, 2, 3])
                }
            ));
            started_rx
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap();
            service.reset(2);
            release_tx.send(()).unwrap();
            let mut v = view(Dimension::Overworld);
            v.session = 2;
            service.request(v);
            let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while service.frame().is_none() {
                assert!(std::time::Instant::now() < until);
                std::thread::yield_now();
            }
            assert!(service.coverage().lod.is_empty());
            assert_eq!(service.frame().unwrap().view.session, 2);
            assert_eq!(service.frame().unwrap().known, 0);
            service.shutdown();
        }

        #[test]
        fn lod_texels_keep_their_resolution_colors_and_opacity_at_every_zoom() {
            let mut cache = Cache::default();
            cache.insert(
                Dimension::Overworld,
                Layer::Surface,
                [10, 0],
                coarse_tile([80, 90, 100]),
            );
            let mut tile = coarse_tile([1, 2, 3]);
            for i in 0..256 {
                tile.pixels[i * 4..i * 4 + 4].copy_from_slice(&[
                    i as u8,
                    (255 - i) as u8,
                    55,
                    if i % 3 == 0 { 0 } else { 255 },
                ]);
            }
            let original = tile.pixels.clone();
            let key = LodKey {
                x: 0,
                z: 0,
                level: 2,
            };
            cache.insert_lod(Dimension::Overworld, key, tile);
            let mut v = view(Dimension::Overworld);
            v.center = [80.0, 32.0];
            v.span = [256.0; 2];
            let levels = cache.terrain_atlas(&v, 1).levels();
            for atlas in levels {
                let draw = &atlas.coarse[0];
                for y in 0..16 {
                    let index = ((draw.uv_origin[1] + y) * atlas.width + draw.uv_origin[0]) * 4;
                    assert_eq!(
                        &atlas.pixels[index..index + 64],
                        &original[y * 64..y * 64 + 64]
                    );
                }
                let (src, _) = atlas
                    .coarse_quad(&v, [0.0, 0.0, 256.0, 256.0], draw, key.bounds())
                    .unwrap();
                assert_eq!(src[2..], [16.0, 16.0]);
                assert!(atlas.width <= 1024 && atlas.height <= 1024);
            }
        }
        #[test]
        fn lod_height_biome_and_finest_source_survive_source_changes_and_camera_turns() {
            let mut cache = Cache::default();
            let key = LodKey {
                x: -1,
                z: 0,
                level: 2,
            };
            let mut tile = coarse_tile([12, 34, 56]);
            tile.heights.fill(87);
            tile.biomes = vec![Some(Arc::from("minecraft:birch_forest")); 256];
            cache.insert_lod(Dimension::Overworld, key, tile);
            let before = cache.coverage();
            let sample = before
                .lod_sample(Dimension::Overworld, Layer::Surface, [-8, 8])
                .unwrap();
            assert_eq!(sample.height, 87);
            assert_eq!(sample.biome.as_deref(), Some("minecraft:birch_forest"));
            assert_eq!(sample.key, key);
            // More than the former LOD budget: do not evict what was behind the camera.
            for x in 0..600 {
                cache.insert_lod(
                    Dimension::Overworld,
                    LodKey { x, z: 10, level: 2 },
                    coarse_tile([90, 80, 70]),
                );
            }
            let mut v = view(Dimension::Overworld);
            v.center = [-8.0, 8.0];
            v.span = [8.0; 2];
            for angle in [0.0, 1.5, 3.0, 5.0] {
                v.angle = angle;
                assert_eq!(cache.render(v.clone(), 1).known, 256);
                assert_eq!(
                    cache
                        .coverage()
                        .lod_sample(Dimension::Overworld, Layer::Surface, [-8, 8])
                        .unwrap()
                        .height,
                    87
                );
            }
            assert_eq!(
                before
                    .lod_sample(Dimension::Overworld, Layer::Surface, [-8, 8])
                    .unwrap()
                    .biome
                    .as_deref(),
                Some("minecraft:birch_forest")
            );
        }
        #[test]
        fn atlas_capacity_limits_only_presentation_not_cached_terrain() {
            let mut cache = Cache::default();
            for x in 0..4100 {
                cache.insert_lod(
                    Dimension::Overworld,
                    LodKey { x, z: 0, level: 0 },
                    coarse_tile([1, 2, 3]),
                );
            }
            let mut near = view(Dimension::Overworld);
            near.span = [16.0; 2];
            near.center = [8.0; 2];
            assert!(cache.gpu_atlas_fits(&near));
            assert!(cache.terrain_atlas(&near, 1).height <= 1024);
            let mut far = near.clone();
            far.span = [150000.0; 2];
            far.width = 1600;
            far.height = 1200;
            assert!(!cache.gpu_atlas_fits(&far));
            let frame = cache.render(far, 2);
            assert!(frame.view.width <= 1024 && frame.view.height <= 1024);
            assert_eq!(frame.pixels.len(), frame.view.width * frame.view.height * 4);
            assert_eq!(cache.lod_tiles.len(), 4100);
            assert!(
                cache
                    .coverage()
                    .lod_sample(Dimension::Overworld, Layer::Surface, [0, 0])
                    .is_some()
            );
            assert!(
                cache
                    .coverage()
                    .lod_sample(Dimension::Overworld, Layer::Surface, [4099 * 16, 0])
                    .is_some()
            );
        }
        #[test]
        fn coarse_atlas_geometry_uses_actual_world_extent_and_valid_uvs() {
            let mut cache = Cache::default();
            let key = LodKey {
                x: -1,
                z: 0,
                level: 3,
            };
            cache.insert_lod(Dimension::Overworld, key, coarse_tile([1, 2, 3]));
            let mut v = view(Dimension::Overworld);
            v.center = [-64.0, 64.0];
            v.span = [128.0; 2];
            let atlas = cache.terrain_atlas(&v, 1);
            let draw = &atlas.coarse[0];
            let (src, dst) = atlas
                .coarse_quad(&v, [10.0, 20.0, 256.0, 256.0], draw, key.bounds())
                .unwrap();
            assert_eq!(src, [0.0, 0.0, 16.0, 16.0]);
            assert_eq!(dst, [10.0, 20.0, 256.0, 256.0]);
            assert_eq!(atlas.chunks.len(), 0);
        }
        #[test]
        fn frame_pixels_map_back_to_their_world_position_at_any_angle() {
            let mut frame = view(Dimension::Overworld);
            frame.center = [100.25, -42.5];
            frame.angle = 0.7;
            for world in [[100.25, -42.5], [103.5, -40.0], [97.0, -45.75]] {
                let pixel = frame.screen_at(world);
                let back = frame.world_at(pixel[0], pixel[1]);
                assert!((back[0] - world[0]).abs() < 1e-9);
                assert!((back[1] - world[1]).abs() < 1e-9);
            }
            assert_eq!(frame.block_at(8.0, 8.0), [100, -43]);
        }
        #[test]
        fn an_underground_slice_does_not_sample_the_bedrock_roof() {
            let tile = sample(
                |_, y, _| Block {
                    rgb: if y == 127 {
                        0x222222
                    } else if y == 32 {
                        0xaa3300
                    } else {
                        0
                    },
                    air: y != 127 && y != 32,
                    alpha: 0,
                    hidden: false,
                    water: false,
                    portal: None,
                },
                |_, _| 128,
                [0, 0],
                0,
                Layer::Below(40),
            );
            assert_eq!(&tile.pixels[..4], &[170, 51, 0, 255]);
        }
        #[test]
        fn water_is_translucent_and_reveals_the_seabed() {
            let tile = sample(
                |_, y, _| match y {
                    40 => Block {
                        rgb: 0xff0000,
                        air: false,
                        alpha: 0,
                        hidden: false,
                        water: false,
                        portal: None,
                    },
                    41..=60 => Block {
                        rgb: 0x0000ff,
                        air: false,
                        alpha: 128,
                        hidden: false,
                        water: false,
                        portal: None,
                    },
                    _ => Block::default(),
                },
                |_, _| 64,
                [0, 0],
                0,
                Layer::Surface,
            );
            assert_eq!(tile.pixels[3], 255);
            assert_eq!(tile.pixels[0], 127);
            assert_eq!(tile.pixels[2], 128);
        }
        #[test]
        fn deeper_water_covers_the_seabed_more() {
            let tile = |depth: i32| {
                sample(
                    |_, y, _| match y {
                        40 => Block {
                            rgb: 0x00ff00,
                            air: false,
                            alpha: 0,
                            hidden: false,
                            water: false,
                            portal: None,
                        },
                        41..=60 if y <= 40 + depth => Block {
                            rgb: 0x0000ff,
                            air: false,
                            alpha: 128,
                            hidden: false,
                            water: true,
                            portal: None,
                        },
                        _ => Block::default(),
                    },
                    |_, _| 64,
                    [0, 0],
                    0,
                    Layer::Surface,
                )
            };
            let shallow = tile(1);
            let deep = tile(20);
            assert_eq!(shallow.heights[0], 40);
            assert_eq!(deep.heights[0], 40);
            assert!(deep.pixels[1] < shallow.pixels[1]);
        }
        #[test]
        fn portals_are_recorded_with_their_block_height() {
            let tile = sample(
                |x, y, z| {
                    if x == 0 && z == 0 && y == 40 {
                        Block {
                            rgb: 0x8a2be2,
                            air: false,
                            alpha: 140,
                            hidden: false,
                            water: false,
                            portal: Some(Portal::Nether),
                        }
                    } else {
                        Block::default()
                    }
                },
                |_, _| 64,
                [0, 0],
                0,
                Layer::Surface,
            );
            assert_eq!(tile.portals, vec![([0, 40, 0], Portal::Nether)]);
        }
        #[test]
        fn stacked_glass_and_water_blend_in_drawing_order() {
            let tile = sample(
                |_, y, _| match y {
                    40 => Block {
                        rgb: 0x102030,
                        air: false,
                        alpha: 0,
                        hidden: false,
                        water: false,
                        portal: None,
                    },
                    41 => Block {
                        rgb: 0x0000ff,
                        air: false,
                        alpha: 128,
                        hidden: false,
                        water: false,
                        portal: None,
                    },
                    42 => Block {
                        rgb: 0xffffff,
                        air: false,
                        alpha: 64,
                        hidden: false,
                        water: false,
                        portal: None,
                    },
                    _ => Block::default(),
                },
                |_, _| 64,
                [0, 0],
                0,
                Layer::Surface,
            );
            let expected = blend(blend(0x102030, 0x0000ff, 128), 0xffffff, 64);
            assert_eq!(
                &tile.pixels[..3],
                &[
                    (expected >> 16) as u8,
                    (expected >> 8) as u8,
                    expected as u8
                ]
            );
        }
        #[test]
        fn a_water_column_without_a_floor_is_not_an_unknown_pixel() {
            let tile = sample(
                |_, y, _| {
                    if (0..=5).contains(&y) {
                        Block {
                            rgb: 0x4040ff,
                            air: false,
                            alpha: 150,
                            hidden: false,
                            water: false,
                            portal: None,
                        }
                    } else {
                        Block::default()
                    }
                },
                |_, _| 6,
                [0, 0],
                0,
                Layer::Surface,
            );
            assert_eq!(tile.pixels[3], 255);
            assert_eq!(&tile.pixels[..3], &[0x40, 0x40, 0xff]);
        }
        #[test]
        fn hidden_ground_cover_does_not_hide_the_terrain() {
            let tile = sample(
                |_, y, _| match y {
                    40 => Block {
                        rgb: 0x8b5a2b,
                        air: false,
                        alpha: 0,
                        hidden: false,
                        water: false,
                        portal: None,
                    },
                    41 => Block {
                        rgb: 0x00ff00,
                        air: false,
                        alpha: 0,
                        hidden: true,
                        water: false,
                        portal: None,
                    },
                    _ => Block::default(),
                },
                |_, _| 42,
                [0, 0],
                0,
                Layer::Surface,
            );
            assert_eq!(&tile.pixels[..3], &[0x8b, 0x5a, 0x2b]);
            assert_eq!(tile.heights[0], 40);
        }
    }
}

#[cfg(test)]
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
            smooth: false,
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
    fn biome_pictograms_classify_ids_without_guessing_unknown_names() {
        assert_eq!(biome_symbol(None), BiomeSymbol::Unknown);
        assert_eq!(biome_symbol(Some("custom:forest")), BiomeSymbol::Unknown);
        assert_eq!(
            biome_symbol(Some("minecraft:crimson_forest")),
            BiomeSymbol::Nether
        );
        assert_eq!(
            biome_symbol(Some("minecraft:snowy_taiga")),
            BiomeSymbol::Snow
        );
        assert_eq!(
            biome_symbol(Some("minecraft:end_highlands")),
            BiomeSymbol::End
        );
        assert_eq!(
            biome_symbol(Some("minecraft:birch_forest")),
            BiomeSymbol::Forest
        );
    }

    #[test]
    fn coordinate_toggles_persist_merge_and_reset_independently() {
        let mut settings = Settings::default();
        let mut edited = settings;
        let mask = minimap::apply(&mut edited, 10);
        minimap::merge(&mut settings, edited, mask);
        assert!(!settings.overworld_coordinates && settings.nether_coordinates);
        minimap::apply(&mut settings, 11);
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
    fn smoothing_defaults_on_toggles_persists_and_resets() {
        let mut settings = Settings::decode("smooth_worldmap=invalid");
        assert!(settings.smooth_worldmap);
        assert_eq!(minimap::apply(&mut settings, 9), 512);
        assert!(!Settings::decode(&settings.encode()).smooth_worldmap);
        minimap::apply(&mut settings, 6);
        assert!(settings.smooth_worldmap);
        assert_eq!(settings.labels().len(), 12);
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
        let frame = wait(&service, |f| f.known == 1);
        let atlas = frame.atlas.as_ref().unwrap();
        assert_eq!(&atlas.pixels[..4], &[0, 255, 0, 255]);
        let mut moved = view(2);
        moved.center = [-31.5, 17.75];
        moved.span = [128.0; 2];
        service.request(moved.clone());
        let next = wait(&service, |f| f.view == moved);
        assert!(Arc::ptr_eq(atlas, next.atlas.as_ref().unwrap()));
        assert!(next.pixels.is_empty());
        assert!(
            service.submit(2, Dimension::Overworld, Layer::Surface, [0, 0], || Tile {
                pixels: [255, 0, 0, 255].repeat(256),
                ..Tile::default()
            })
        );
        let changed = wait(&service, |f| {
            f.atlas.as_ref().is_some_and(|a| a.serial != atlas.serial)
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
