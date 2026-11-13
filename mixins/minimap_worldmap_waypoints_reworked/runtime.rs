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

    /// What a waypoint's 3D-world marker shows. The coloured marker (its first
    /// letter or a skull) is always drawn; the label below it is optional.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub enum WorldMarkerStyle {
        #[default]
        LetterDistance,
        Letter,
        NameDistance,
    }

    impl WorldMarkerStyle {
        pub const ALL: [Self; 3] = [Self::LetterDistance, Self::Letter, Self::NameDistance];

        pub fn name(self) -> &'static str {
            match self {
                Self::LetterDistance => "letter_distance",
                Self::Letter => "letter",
                Self::NameDistance => "name_distance",
            }
        }

        fn from_name(name: &str) -> Option<Self> {
            match name {
                "letter_distance" => Some(Self::LetterDistance),
                "letter" => Some(Self::Letter),
                "name_distance" => Some(Self::NameDistance),
                _ => None,
            }
        }

        pub fn label(self) -> &'static str {
            match self {
                Self::LetterDistance => tr(
                    "Letter + distance",
                    "Буква + дистанция",
                    "Lettre + distance",
                    "Letra + distancia",
                    "Buchstabe + Entfernung",
                ),
                Self::Letter => tr(
                    "Letter only",
                    "Только буква",
                    "Lettre seule",
                    "Solo letra",
                    "Nur Buchstabe",
                ),
                Self::NameDistance => tr(
                    "Name + distance",
                    "Имя + дистанция",
                    "Nom + distance",
                    "Nombre + distancia",
                    "Name + Entfernung",
                ),
            }
        }
    }

    /// How the map's direction labels are written.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub enum DirectionStyle {
        #[default]
        Compass,
        Axes,
    }

    impl DirectionStyle {
        pub const ALL: [Self; 2] = [Self::Compass, Self::Axes];

        pub fn name(self) -> &'static str {
            match self {
                Self::Compass => "compass",
                Self::Axes => "axes",
            }
        }

        fn from_name(name: &str) -> Option<Self> {
            match name {
                "compass" => Some(Self::Compass),
                "axes" => Some(Self::Axes),
                _ => None,
            }
        }

        pub fn label(self) -> &'static str {
            match self {
                Self::Compass => tr("Compass", "Компас", "Boussole", "Brújula", "Kompass"),
                Self::Axes => tr("Axes", "Оси", "Axes", "Ejes", "Achsen"),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Settings {
        pub cache_ram_mib: u16,
        pub enabled: bool,
        pub corner: Corner,
        pub size_px: u16,
        pub blocks_per_pixel: f32,
        pub rotate_with_player: bool,
        pub round: bool,
        pub mini_dimension: super::Dimension,
        pub world_dimension: super::Dimension,
        pub overworld_coordinates: bool,
        pub nether_coordinates: bool,
        pub death_waypoints: bool,
        pub death_waypoint_auto_remove: bool,
        pub world_waypoints: bool,
        pub world_waypoint_style: WorldMarkerStyle,
        pub directions: bool,
        pub direction_style: DirectionStyle,
    }

    impl Default for Settings {
        fn default() -> Self {
            Self {
                cache_ram_mib: 64,
                enabled: true,
                corner: Corner::TopRight,
                size_px: 128,
                blocks_per_pixel: 1.0,
                rotate_with_player: false,
                round: false,
                mini_dimension: super::Dimension::Player,
                world_dimension: super::Dimension::Player,
                overworld_coordinates: true,
                nether_coordinates: true,
                death_waypoints: true,
                death_waypoint_auto_remove: true,
                world_waypoints: true,
                world_waypoint_style: WorldMarkerStyle::default(),
                directions: false,
                direction_style: DirectionStyle::default(),
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
                    "death_waypoints" => {
                        value.death_waypoints = raw.parse().unwrap_or(value.death_waypoints)
                    }
                    "death_waypoint_auto_remove" => {
                        value.death_waypoint_auto_remove =
                            raw.parse().unwrap_or(value.death_waypoint_auto_remove)
                    }
                    "world_waypoints" => {
                        value.world_waypoints = raw.parse().unwrap_or(value.world_waypoints)
                    }
                    "world_waypoint_style" => {
                        if let Some(style) = WorldMarkerStyle::from_name(raw) {
                            value.world_waypoint_style = style;
                        }
                    }
                    "directions" => value.directions = raw.parse().unwrap_or(value.directions),
                    "direction_style" => {
                        if let Some(style) = DirectionStyle::from_name(raw) {
                            value.direction_style = style;
                        }
                    }
                    "cache_ram_mib" => {
                        if let Ok(n) = raw.parse::<u16>()
                            && (16..=512).contains(&n)
                        {
                            value.cache_ram_mib = n;
                        }
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
                "# Minimap Reworked settings\nenabled={}\ncorner={}\nsize={}\nshape={}\nzoom={}\nrotate={}\nmini_dimension={}\nworld_dimension={}\noverworld_coordinates={}\nnether_coordinates={}\ndeath_waypoints={}\ndeath_waypoint_auto_remove={}\nworld_waypoints={}\nworld_waypoint_style={}\ndirections={}\ndirection_style={}\ncache_ram_mib={}\n",
                self.enabled,
                self.corner.name(),
                self.size_px,
                if self.round { "round" } else { "square" },
                self.blocks_per_pixel,
                self.rotate_with_player,
                self.mini_dimension.name(),
                self.world_dimension.name(),
                self.overworld_coordinates,
                self.nether_coordinates,
                self.death_waypoints,
                self.death_waypoint_auto_remove,
                self.world_waypoints,
                self.world_waypoint_style.name(),
                self.directions,
                self.direction_style.name(),
                self.cache_ram_mib,
            )
        }

        // Returns the changed field mask. Unknown buttons are harmless.
        fn click(&mut self, button: usize) -> u32 {
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
                    return 0x3ffbf;
                }
                7 => {
                    self.mini_dimension = super::cycle(self.mini_dimension, &super::Dimension::ALL)
                }
                8 => {
                    self.world_dimension =
                        super::cycle(self.world_dimension, &super::Dimension::ALL)
                }
                9 => self.overworld_coordinates = !self.overworld_coordinates,
                10 => self.nether_coordinates = !self.nether_coordinates,
                11 => self.death_waypoints = !self.death_waypoints,
                12 => self.death_waypoint_auto_remove = !self.death_waypoint_auto_remove,
                13 => self.world_waypoints = !self.world_waypoints,
                14 => {
                    self.world_waypoint_style =
                        super::cycle(self.world_waypoint_style, &WorldMarkerStyle::ALL)
                }
                15 => self.directions = !self.directions,
                16 => {
                    self.direction_style = super::cycle(self.direction_style, &DirectionStyle::ALL)
                }
                _ => return 0,
            }
            1 << button
        }

        // Only locally edited fields override settings loaded asynchronously.
        fn overlay(&mut self, edited: Self, mask: u32) {
            if mask & (1 << 17) != 0 {
                self.cache_ram_mib = edited.cache_ram_mib;
            }
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
                self.overworld_coordinates = edited.overworld_coordinates;
            }
            if mask & 1024 != 0 {
                self.nether_coordinates = edited.nether_coordinates;
            }
            if mask & 2048 != 0 {
                self.death_waypoints = edited.death_waypoints;
            }
            if mask & 4096 != 0 {
                self.death_waypoint_auto_remove = edited.death_waypoint_auto_remove;
            }
            if mask & 8192 != 0 {
                self.world_waypoints = edited.world_waypoints;
            }
            if mask & 16384 != 0 {
                self.world_waypoint_style = edited.world_waypoint_style;
            }
            if mask & 32768 != 0 {
                self.directions = edited.directions;
            }
            if mask & 65536 != 0 {
                self.direction_style = edited.direction_style;
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
                localized_format!(
                    "Death waypoints: {}",
                    "Вейпоинты смерти: {}",
                    "Repères de mort : {}",
                    "Marcadores de muerte: {}",
                    "Todes-Wegpunkte: {}",
                    on(self.death_waypoints)
                ),
                localized_format!(
                    "Remove death waypoints when reached: {}",
                    "Удалять вейпоинты смерти при приближении: {}",
                    "Supprimer les repères de mort à l’approche : {}",
                    "Eliminar marcadores de muerte al acercarse: {}",
                    "Todes-Wegpunkte bei Annäherung entfernen: {}",
                    on(self.death_waypoint_auto_remove)
                ),
                localized_format!(
                    "Show waypoints in the 3D world: {}",
                    "Показывать вейпоинты в 3D-мире: {}",
                    "Afficher les repères dans le monde 3D : {}",
                    "Mostrar marcadores en el mundo 3D: {}",
                    "Wegpunkte in der 3D-Welt zeigen: {}",
                    on(self.world_waypoints)
                ),
                localized_format!(
                    "3D marker: {}",
                    "3D-маркер: {}",
                    "Marqueur 3D : {}",
                    "Marcador 3D: {}",
                    "3D-Markierung: {}",
                    self.world_waypoint_style.label()
                ),
                localized_format!(
                    "Direction letters: {}",
                    "Буквы направлений: {}",
                    "Lettres de direction : {}",
                    "Letras de dirección: {}",
                    "Richtungsbuchstaben: {}",
                    on(self.directions)
                ),
                localized_format!(
                    "Direction style: {}",
                    "Стиль направлений: {}",
                    "Style de direction : {}",
                    "Estilo de dirección: {}",
                    "Richtungsstil: {}",
                    self.direction_style.label()
                ),
            ]
        }

        pub fn fitted_size(self, width: f32, height: f32) -> Option<f32> {
            // Horizon clamps its minimap to at least 16 GUI pixels. Hide if that cannot fit.
            let available = (width - 16.0).min(height - 64.0);
            (available >= 16.0).then(|| f32::from(self.size_px).min(available))
        }
    }

    pub(super) fn apply(settings: &mut Settings, button: usize) -> u32 {
        settings.click(button)
    }
    pub(super) fn merge(settings: &mut Settings, edited: Settings, mask: u32) {
        settings.overlay(edited, mask);
    }
}

use minimap::Settings;

/// Original 16×16 biome pictograms. Each row packs sixteen two-bit palette indices,
/// leftmost pixel first; index zero is transparent. Colours are portable ARGB.
#[derive(Debug, PartialEq, Eq)]
pub struct BiomePictogram {
    pub rows: [u32; 16],
    pub colors: [u32; 3],
}

pub const BIOME_ICON_SIZE: usize = 16;

pub fn biome_pictogram(id: Option<&str>) -> &'static BiomePictogram {
    id.and_then(|id| id.strip_prefix("minecraft:"))
        .and_then(|name| BIOME_PICTOGRAMS.iter().find(|(key, _)| *key == name))
        .map_or(&UNKNOWN_BIOME_PICTOGRAM, |(_, icon)| icon)
}

static UNKNOWN_BIOME_PICTOGRAM: BiomePictogram = BiomePictogram {
    rows: [
        0, 0x00555000, 0x01400500, 0x05000140, 0x00000140, 0x00000500, 0x00001400, 0x00005000,
        0x00005000, 0, 0, 0x00005000, 0x00005000, 0, 0, 0,
    ],
    colors: [0xffaeb9c6, 0, 0],
};

pub static BIOME_PICTOGRAMS: &[(&str, BiomePictogram)] = &[
    (
        "badlands",
        BiomePictogram {
            rows: [
                0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x01555400, 0x01555400, 0x05555400,
                0x05555400, 0x05555554, 0x0ffffffc, 0x15555554, 0x15555554, 0x2aaaaaa8, 0x55555554,
                0x55555554, 0x00000000,
            ],
            colors: [0xffc37b43, 0xffb34f38, 0xffe9c78e],
        },
    ),
    (
        "bamboo_jungle",
        BiomePictogram {
            rows: [
                0x00053000, 0x00c5c000, 0x003b0000, 0x000d014c, 0x14c53170, 0x17c5cec0, 0xec3b0340,
                0x340d014c, 0x14c53170, 0x17c5cec0, 0xec3b0340, 0x340d014c, 0x14053170, 0x14050ec0,
                0x14050340, 0x00000000,
            ],
            colors: [0xff4e9b4b, 0xffc4ce6a, 0xff267348],
        },
    ),
    (
        "basalt_deltas",
        BiomePictogram {
            rows: [
                0x00000000, 0x00000000, 0x00055000, 0x00059000, 0x00059000, 0x15459000, 0x16459000,
                0x16459154, 0x16459164, 0x16459164, 0x16459164, 0x16455164, 0x16755164, 0x16755154,
                0x15757f54, 0x00303f00,
            ],
            colors: [0xff676672, 0xffa4a2a5, 0xffef963e],
        },
    ),
    (
        "beach",
        BiomePictogram {
            rows: [
                0xaaa15555, 0xaaa15555, 0xaa815555, 0xaa855555, 0xaa855555, 0xaaa15555, 0xaaa15555,
                0xaaa857d5, 0xaaa85ff5, 0xaaaa1ff5, 0xaaaa17d5, 0xaaa85555, 0xaaa85ff5, 0xaaa85555,
                0xaaa15555, 0xaaa15555,
            ],
            colors: [0xffe8cd8d, 0xff69bce1, 0xfff9edbf],
        },
    ),
    (
        "birch_forest",
        BiomePictogram {
            rows: [
                0x00055000, 0x00155400, 0x01555500, 0x05555540, 0x15555550, 0x15555550, 0x15555550,
                0x05555540, 0x01555500, 0x0017d000, 0x000a8000, 0x000a8000, 0x000bc000, 0x000a8000,
                0x000a8000, 0x00000000,
            ],
            colors: [0xff96bb56, 0xffe8e3ce, 0xff50554f],
        },
    ),
    (
        "cherry_grove",
        BiomePictogram {
            rows: [
                0x00055000, 0x00155400, 0x01fd5500, 0x05fd5540, 0x15555fd0, 0x15555fd0, 0x15555550,
                0x05555540, 0x01555500, 0x00155000, 0x000a8000, 0x050a8000, 0x000a8000, 0x000a8014,
                0x000a8000, 0x00000000,
            ],
            colors: [0xffeca5c4, 0xff80565c, 0xffffd5e5],
        },
    ),
    (
        "cold_ocean",
        BiomePictogram {
            rows: [
                0x00000000, 0x00000000, 0x00000080, 0x00054080, 0x10101aa8, 0x05400590, 0x00000080,
                0x00000000, 0x00054000, 0x10101004, 0x05400550, 0x00000000, 0x00000000, 0x00000000,
                0x00000000, 0x00000000,
            ],
            colors: [0xff608fbb, 0xffa6d5e9, 0xff426785],
        },
    ),
    (
        "crimson_forest",
        BiomePictogram {
            rows: [
                0x00000000, 0x00555400, 0x01555500, 0x05555f50, 0x15555f50, 0x15f55550, 0x15f55554,
                0x15555554, 0x000a8000, 0x000a8000, 0x000a8000, 0x000a8000, 0x150a8000, 0x150a8154,
                0x150a8154, 0x00000000,
            ],
            colors: [0xffb43e5e, 0xff79464c, 0xfff3ad78],
        },
    ),
    (
        "dappled_forest",
        BiomePictogram {
            rows: [
                0x00055000, 0x00155400, 0x015f5500, 0x055f57c0, 0x155557d0, 0x17d55550, 0x17d57d50,
                0x05557d40, 0x01555500, 0x0017d000, 0x000a8000, 0x000a8000, 0x000a8000, 0x000a8000,
                0x000a8000, 0x00000000,
            ],
            colors: [0xffcf5f21, 0xff6f4a2f, 0xffa4281f],
        },
    ),
    (
        "dark_forest",
        BiomePictogram {
            rows: [
                0x00055000, 0x00155400, 0x05555500, 0x15555550, 0x55555554, 0x55555555, 0x55555555,
                0x55555554, 0x15555550, 0x05555000, 0x000a8000, 0x000a8ffc, 0x000a8ffc, 0x000a82a0,
                0x000a82a0, 0x00000000,
            ],
            colors: [0xff355c3d, 0xff795338, 0xff52754a],
        },
    ),
    (
        "deep_cold_ocean",
        BiomePictogram {
            rows: [
                0x00000000, 0x00000000, 0x00000080, 0x00054080, 0x10101aa8, 0x05400590, 0x00000080,
                0x00000000, 0x00054000, 0x10121004, 0x05622550, 0x000a8000, 0x000a8000, 0x00020000,
                0x0ffffff0, 0x00000000,
            ],
            colors: [0xff3c608c, 0xff7eafc9, 0xff293f63],
        },
    ),
    (
        "deep_dark",
        BiomePictogram {
            rows: [
                0x00000000, 0x00400000, 0x01555500, 0x01555540, 0x07555740, 0x076aa750, 0x17aaab50,
                0x17aaab54, 0x16efae54, 0x16aaaa54, 0x17eaaa54, 0x16aaaf54, 0x16abea54, 0x16aaaa54,
                0x16aaaa54, 0x00000000,
            ],
            colors: [0xff255a61, 0xff132b39, 0xff61d3cf],
        },
    ),
    (
        "deep_frozen_ocean",
        BiomePictogram {
            rows: [
                0x00800000, 0x00800200, 0x02a00200, 0x02a54a80, 0x1aa81a84, 0x0aa80a90, 0x2aaa2aa0,
                0x00002aa0, 0x00054000, 0x10121004, 0x05622550, 0x000a8000, 0x000a8000, 0x00020000,
                0x0ffffff0, 0x00000000,
            ],
            colors: [0xff456b96, 0xffc6e7f3, 0xff2e4e72],
        },
    ),
    (
        "deep_lukewarm_ocean",
        BiomePictogram {
            rows: [
                0x00000000, 0x00000000, 0x00000000, 0x00054000, 0x10101004, 0x05400550, 0x00000000,
                0x00000020, 0x08054020, 0x18121084, 0x06622590, 0x020a8080, 0x080a8020, 0x08020020,
                0x0ffffff0, 0x00000000,
            ],
            colors: [0xff388aa9, 0xff92c4a4, 0xff24627f],
        },
    ),
    (
        "deep_ocean",
        BiomePictogram {
            rows: [
                0x00000000, 0x02a00000, 0x00000000, 0x00054000, 0x10101004, 0x05400550, 0x00000000,
                0x00000000, 0x00054000, 0x10121004, 0x05622550, 0x000a8a80, 0x000a8000, 0x00020000,
                0x0ffffff0, 0x00000000,
            ],
            colors: [0xff346d9f, 0xff6aa7cb, 0xff254b73],
        },
    ),
    (
        "desert",
        BiomePictogram {
            rows: [
                0x00000000, 0x000003f0, 0x00000ffc, 0x00000ffc, 0x00000ffc, 0x0a2803f0, 0x0a280000,
                0x0a28a000, 0x0aa8a000, 0x0aaaa000, 0x002aa000, 0x00280000, 0x00280000, 0x15555554,
                0x15555554, 0x00000000,
            ],
            colors: [0xffdabb78, 0xff68964f, 0xfff9d764],
        },
    ),
    (
        "dripstone_caves",
        BiomePictogram {
            rows: [
                0x00000000, 0x00400000, 0x01555500, 0x01555540, 0x0555fd40, 0x056afd50, 0x15aafd50,
                0x15aab954, 0x16aaba54, 0x16baba54, 0x16baba54, 0x16faaa54, 0x16feaa54, 0x17feaa54,
                0x17feaa54, 0x00000000,
            ],
            colors: [0xffac866b, 0xff38313c, 0xffd1aa81],
        },
    ),
    (
        "end_barrens",
        BiomePictogram {
            rows: [
                0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x04000000,
                0x15554000, 0x556a9554, 0x05556a50, 0x0157d540, 0x0017d400, 0x00055000, 0x00004000,
                0x00000000, 0x00000000,
            ],
            colors: [0xffbcb98e, 0xffddd9ac, 0xff8c799f],
        },
    ),
    (
        "end_highlands",
        BiomePictogram {
            rows: [
                0x0003c000, 0x0003c000, 0x0003ff00, 0x0fc3ff00, 0x00c00f00, 0x00c00f00, 0x00c00f00,
                0x01d55f00, 0x05555500, 0x15555550, 0x5aaaaaa4, 0x15555550, 0x05555540, 0x01555500,
                0x00555400, 0x00155000,
            ],
            colors: [0xffccc88d, 0xffeae3ad, 0xffa17cc3],
        },
    ),
    (
        "end_midlands",
        BiomePictogram {
            rows: [
                0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x002aa800, 0x016aa800, 0x05555500,
                0x15555550, 0x5aaaaaa4, 0x15555550, 0x05555540, 0x015fd500, 0x005fd400, 0x00155000,
                0x00000000, 0x00000000,
            ],
            colors: [0xffc7c28d, 0xffe3dfb2, 0xff9982b6],
        },
    ),
    (
        "eroded_badlands",
        BiomePictogram {
            rows: [
                0x00000000, 0x00054000, 0x00054000, 0x00054000, 0x00054000, 0x00054150, 0x00054150,
                0x15054150, 0x15155150, 0x3f3ff3f0, 0x15155554, 0x55555554, 0xaaaaaaa8, 0x55555554,
                0x55555554, 0x00000000,
            ],
            colors: [0xffca8953, 0xffaa5039, 0xfff0d3a2],
        },
    ),
    (
        "flower_forest",
        BiomePictogram {
            rows: [
                0x00055000, 0x00155400, 0x01555500, 0x05555540, 0x15555550, 0x15555550, 0x15555550,
                0x05555540, 0x01555500, 0x00155000, 0x000a8000, 0x000a80f0, 0x0f0a80f0, 0x0f0aa800,
                0x000aa800, 0x00000000,
            ],
            colors: [0xff539e62, 0xffec9bb8, 0xfff5d06b],
        },
    ),
    (
        "forest",
        BiomePictogram {
            rows: [
                0x00055000, 0x00155400, 0x01555500, 0x05555540, 0x15555550, 0x15555550, 0x15555550,
                0x05555540, 0x01555500, 0x00155000, 0x000a8000, 0x000a8000, 0x000a8000, 0x000a8000,
                0x000a8000, 0x00000000,
            ],
            colors: [0xff4ca45c, 0xff91653f, 0xff86c16b],
        },
    ),
    (
        "frozen_ocean",
        BiomePictogram {
            rows: [
                0x00800000, 0x00800200, 0x02a00200, 0x02a54a80, 0x1aa81a84, 0x0aa80a90, 0x2aaa2aa0,
                0x00002aa0, 0x00054000, 0x10101004, 0x05400550, 0x00000000, 0x00000000, 0x00000000,
                0x00000000, 0x00000000,
            ],
            colors: [0xff77adcd, 0xffe3f9ff, 0xff52758e],
        },
    ),
    (
        "frozen_peaks",
        BiomePictogram {
            rows: [
                0x00000000, 0x00020000, 0x00068000, 0x000a8000, 0x001aa000, 0x002ba000, 0x006ba800,
                0x00ae9800, 0x015d9600, 0x01656500, 0x05b56540, 0x05956540, 0x16d55950, 0x16555950,
                0x57555554, 0x00000000,
            ],
            colors: [0xff88c9e6, 0xffe9fbff, 0xff498ba8],
        },
    ),
    (
        "frozen_river",
        BiomePictogram {
            rows: [
                0x01555400, 0x00556400, 0x00569400, 0x00195400, 0x00165400, 0x00059500, 0x00016540,
                0x00005950, 0x00001554, 0x00000555, 0x00001594, 0x00005654, 0x00005950, 0x00016550,
                0x00055540, 0x00155540,
            ],
            colors: [0xffa0d9ef, 0xffe6f8ff, 0xff678ca7],
        },
    ),
    (
        "grove",
        BiomePictogram {
            rows: [
                0x00000000, 0x00400000, 0x01500000, 0x01500400, 0x05f41500, 0x0f5f1500, 0x3555d540,
                0x15555540, 0x55555550, 0x55555550, 0x55555554, 0x55555555, 0x02a55555, 0x02a55555,
                0x3ffffffc, 0x3ffffffc,
            ],
            colors: [0xff679587, 0xff937359, 0xffe3f3ef],
        },
    ),
    (
        "ice_spikes",
        BiomePictogram {
            rows: [
                0x00080000, 0x00080000, 0x000a0000, 0x000a0100, 0x002e0100, 0x042e0140, 0x042e8540,
                0x052e8540, 0x152e8540, 0x152ba550, 0x156ba550, 0x156ba550, 0x556ba550, 0x555ba954,
                0x555aa954, 0x00000000,
            ],
            colors: [0xff92cfe8, 0xffe5faff, 0xff4d86ae],
        },
    ),
    (
        "jagged_peaks",
        BiomePictogram {
            rows: [
                0x00000000, 0x00020000, 0x00068000, 0x000a8000, 0x001aa000, 0x002ba080, 0x006ba880,
                0x00ae5aa0, 0x015d56a0, 0x01755ff8, 0x05755ffc, 0x05d57ffc, 0x15d57fff, 0x1755ffff,
                0x57555554, 0x00000000,
            ],
            colors: [0xff8b969f, 0xfff2f8fc, 0xff56626d],
        },
    ),
    (
        "jungle",
        BiomePictogram {
            rows: [
                0x00155400, 0x01555500, 0x05555540, 0x15555554, 0x15555555, 0x17555555, 0x07555555,
                0x03555555, 0x03155555, 0x03055555, 0x03095555, 0x030a5555, 0x030a8554, 0x030a82a0,
                0x000a82a0, 0x00000000,
            ],
            colors: [0xff31834b, 0xff8b6945, 0xff75ba4c],
        },
    ),
    (
        "lukewarm_ocean",
        BiomePictogram {
            rows: [
                0x00000000, 0x00000000, 0x00000000, 0x00054000, 0x10101004, 0x05400550, 0x00000000,
                0x00000020, 0x08054020, 0x18101084, 0x06400590, 0x02000080, 0x08000020, 0x08000020,
                0x00000000, 0x00000000,
            ],
            colors: [0xff55bbd9, 0xffb0dbaf, 0xff3389b9],
        },
    ),
    (
        "lush_caves",
        BiomePictogram {
            rows: [
                0x00000000, 0x00400000, 0x01555500, 0x01555540, 0x05555540, 0x056e9550, 0x15bf9950,
                0x15ae9954, 0x16a69a54, 0x16a6aa54, 0x16a6aa54, 0x16a6aa54, 0x16aaaa54, 0x16555554,
                0x16555554, 0x00000000,
            ],
            colors: [0xff719e57, 0xff303e39, 0xffe8a7cb],
        },
    ),
    (
        "mangrove_swamp",
        BiomePictogram {
            rows: [
                0x00055000, 0x00155400, 0x01555500, 0x05555540, 0x15569550, 0x15569550, 0x15555550,
                0x05555540, 0x01555540, 0x01155040, 0x010a8040, 0x002aa000, 0x3f8a88fc, 0x008a8800,
                0x2a0a82a0, 0x3ffffffc,
            ],
            colors: [0xff568751, 0xff875144, 0xff75b7a2],
        },
    ),
    (
        "meadow",
        BiomePictogram {
            rows: [
                0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000540, 0x00001550, 0x00001650,
                0x01501550, 0x05540540, 0x05940300, 0x05540300, 0x01530330, 0x00c33330, 0x00ffffc0,
                0x3ffffffc, 0xffffffff,
            ],
            colors: [0xff91ba5d, 0xfff5d8ea, 0xff659c47],
        },
    ),
    (
        "mushroom_fields",
        BiomePictogram {
            rows: [
                0x00000000, 0x00155000, 0x01555500, 0x05556940, 0x15a56950, 0x15a55550, 0x05555540,
                0x05555540, 0x05555540, 0x000a8000, 0x000a8000, 0x000a8000, 0x000a8000, 0x000a8000,
                0x3ffffff0, 0x00000000,
            ],
            colors: [0xffce6468, 0xfff4e7cd, 0xff957b98],
        },
    ),
    (
        "nether_wastes",
        BiomePictogram {
            rows: [
                0x00000000, 0x00000000, 0x00000000, 0x00040000, 0x0a940000, 0x0a950100, 0x15550140,
                0x15554550, 0x15554554, 0x15555554, 0x15f55554, 0x3ffd7754, 0x357fffd4, 0x155fd5fc,
                0x15575574, 0x00000000,
            ],
            colors: [0xffc4594c, 0xff803a3b, 0xffffad4c],
        },
    ),
    (
        "ocean",
        BiomePictogram {
            rows: [
                0x00000000, 0x02a00000, 0x00000000, 0x00054000, 0x10101004, 0x05400550, 0x00000000,
                0x00000000, 0x00054000, 0x10101004, 0x05400550, 0x00000a80, 0x00000000, 0x00000000,
                0x00000000, 0x00000000,
            ],
            colors: [0xff529dce, 0xff8bd0eb, 0xff32759f],
        },
    ),
    (
        "old_growth_birch_forest",
        BiomePictogram {
            rows: [
                0x00155400, 0x00555500, 0x01555540, 0x05555540, 0x15555550, 0x15555550, 0x15569550,
                0x0557d540, 0x01569500, 0x00169000, 0x000a8000, 0x000a8000, 0x000a8000, 0x000a8000,
                0x000a8000, 0x00000000,
            ],
            colors: [0xff87b453, 0xffe8e4d1, 0xff4e5945],
        },
    ),
    (
        "old_growth_pine_taiga",
        BiomePictogram {
            rows: [
                0x00000000, 0x00010000, 0x00054000, 0x00054000, 0x00155000, 0x00155000, 0x00555400,
                0x005a9400, 0x015a9500, 0x055a9540, 0x055a9540, 0x155a9550, 0x000a83fc, 0x000a8ff0,
                0x000a8ff0, 0x00000000,
            ],
            colors: [0xff597a4a, 0xff886646, 0xffa1a15d],
        },
    ),
    (
        "old_growth_spruce_taiga",
        BiomePictogram {
            rows: [
                0x00000000, 0x00010400, 0x00055500, 0x00455500, 0x01555540, 0x01555540, 0x05555550,
                0x05555550, 0x15555554, 0x15555555, 0x55555555, 0x55555555, 0x55555a00, 0x55555600,
                0x3ffffffc, 0x00000000,
            ],
            colors: [0xff3f694e, 0xff806243, 0xff88a15e],
        },
    ),
    (
        "pale_garden",
        BiomePictogram {
            rows: [
                0x00055000, 0x00155400, 0x01555500, 0x05555540, 0x15555550, 0x15555550, 0x15555550,
                0x05555540, 0x01555540, 0x01155040, 0x010a8040, 0x000ec040, 0x000a8000, 0x000a8000,
                0x000a8000, 0x00000000,
            ],
            colors: [0xffb6b9a5, 0xffbcb3a1, 0xffe69d54],
        },
    ),
    (
        "plains",
        BiomePictogram {
            rows: [
                0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00400040,
                0x00400040, 0x01100110, 0x01100110, 0x01340300, 0x03330330, 0x00c33330, 0x00c0c0c0,
                0x3ffffffc, 0x00000000,
            ],
            colors: [0xffa6d45c, 0xfffff1bb, 0xff548f46],
        },
    ),
    (
        "river",
        BiomePictogram {
            rows: [
                0x01555400, 0x00555400, 0x0057d4a8, 0x001574a8, 0x001554a8, 0x000555a8, 0x00015540,
                0x00005550, 0x00001554, 0x2aa00555, 0x2aa01554, 0x2aa05554, 0x2aa05550, 0x2aa15550,
                0x00055540, 0x00155540,
            ],
            colors: [0xff69b7e9, 0xff9dbe66, 0xff447eab],
        },
    ),
    (
        "savanna",
        BiomePictogram {
            rows: [
                0x00000000, 0x00000000, 0x00000000, 0x00015554, 0x00015554, 0x15555554, 0x1555a000,
                0x00028000, 0x00028000, 0x00028000, 0x00028000, 0x00028000, 0x00028000, 0x3ffffff0,
                0x3ffffff0, 0x00000000,
            ],
            colors: [0xff9da44e, 0xff8c6944, 0xffd8c875],
        },
    ),
    (
        "savanna_plateau",
        BiomePictogram {
            rows: [
                0x00000000, 0x00000000, 0x00000000, 0x00015554, 0x00015554, 0x15555554, 0x1555a000,
                0x00028000, 0x00028000, 0x03fff000, 0x03fff000, 0x0fffff00, 0x0fffff00, 0x3ffffff0,
                0x3ffffff0, 0x00000000,
            ],
            colors: [0xffa6a44a, 0xff876047, 0xffcdb17a],
        },
    ),
    (
        "small_end_islands",
        BiomePictogram {
            rows: [
                0x00000000, 0x00000000, 0x00f00000, 0x00f00040, 0x00000150, 0x00001555, 0x00000554,
                0x02000150, 0x0a800040, 0x2aa80000, 0xaaaa0000, 0x2aa80000, 0x0aa00000, 0x0a800000,
                0x02000000, 0x00000000,
            ],
            colors: [0xffd9d4a0, 0xfff2eabd, 0xff9b7ebf],
        },
    ),
    (
        "snowy_beach",
        BiomePictogram {
            rows: [
                0xaaa15555, 0xaaa157f5, 0xaa8157f5, 0xaa855555, 0xaa857ffd, 0xaaa15555, 0xaaa15555,
                0xaaa85555, 0xaaa85555, 0xaaaa1ff5, 0xaaaa1555, 0xaaa85555, 0xaaa85555, 0xaaa85555,
                0xaaa15555, 0xaaa15555,
            ],
            colors: [0xffe1eff3, 0xff78b3d5, 0xffa6b9c3],
        },
    ),
    (
        "snowy_plains",
        BiomePictogram {
            rows: [
                0x00000000, 0x00000000, 0x00000000, 0x00000080, 0x00000080, 0x00800aa8, 0x00000080,
                0x00000080, 0x00000000, 0x00002000, 0x0000a800, 0x000aaa80, 0x156aaaa4, 0x15555554,
                0x15555554, 0x00000000,
            ],
            colors: [0xffd7efff, 0xffffffff, 0xff92b8ce],
        },
    ),
    (
        "snowy_slopes",
        BiomePictogram {
            rows: [
                0x00000000, 0x00100000, 0x00140000, 0x00550000, 0x00550000, 0x01654000, 0x01695000,
                0x016a5400, 0x05aa9400, 0x05aea500, 0x05faa540, 0x3faaa950, 0x16aaaa50, 0x56aaaa94,
                0x56aaaaa5, 0x00000000,
            ],
            colors: [0xffdcebf2, 0xffffffff, 0xff8ba4b7],
        },
    ),
    (
        "snowy_taiga",
        BiomePictogram {
            rows: [
                0x00000000, 0x00030000, 0x000dc000, 0x00054000, 0x001fd000, 0x00f57c00, 0x00555400,
                0x005ff400, 0x03f55f00, 0x05555540, 0x05555540, 0x15555550, 0x000a8000, 0x000a8000,
                0x000a8000, 0x00000000,
            ],
            colors: [0xff5c8c80, 0xff87694d, 0xffe3f4f5],
        },
    ),
    (
        "soul_sand_valley",
        BiomePictogram {
            rows: [
                0x00000000, 0x0c000300, 0x0c0003c0, 0x0f0003c0, 0x3f000ff0, 0x3fcaaff0, 0x302aaff0,
                0x009696f0, 0x00969630, 0x002aa800, 0x155aa554, 0x15555554, 0x15555554, 0x15555554,
                0x15555554, 0x00000000,
            ],
            colors: [0xff8f7766, 0xff594f4e, 0xff72d9d5],
        },
    ),
    (
        "sparse_jungle",
        BiomePictogram {
            rows: [
                0x00155400, 0x01555500, 0x05555540, 0x15555550, 0x155555d0, 0x155555d0, 0x055555c0,
                0x015555c0, 0x001550c0, 0x0c0a80c0, 0x3c0a80c0, 0x330a80c0, 0x000a80c0, 0x000a80fc,
                0x000a80fc, 0x00000000,
            ],
            colors: [0xff70a557, 0xff917348, 0xffa5c76d],
        },
    ),
    (
        "stony_peaks",
        BiomePictogram {
            rows: [
                0x00000000, 0x00010000, 0x00054000, 0x00054000, 0x00155000, 0x00169000, 0x00569400,
                0x005d5400, 0x015d5500, 0x01755500, 0x05755a40, 0x05d55a40, 0x16a55550, 0x17555550,
                0x57555554, 0x00000000,
            ],
            colors: [0xffa5a196, 0xffcec8b9, 0xff6d7169],
        },
    ),
    (
        "stony_shore",
        BiomePictogram {
            rows: [
                0xaaa15555, 0xaaa15555, 0xaa8157f5, 0xaa855ffd, 0xaa855ffd, 0xaaa1f7f5, 0xaaa1f555,
                0xaaa85555, 0xaaa85555, 0xaaaa17d5, 0xaaaa1ff5, 0xaaa85ff5, 0xaaa857d5, 0xaaa85555,
                0xaaa15555, 0xaaa15555,
            ],
            colors: [0xff949c9e, 0xff70b3d1, 0xffd0d2c9],
        },
    ),
    (
        "sulfur_caves",
        BiomePictogram {
            rows: [
                0x00000000, 0x00400000, 0x01555500, 0x01555540, 0x057d5540, 0x056aa550, 0x15baa950,
                0x15baa954, 0x16feae54, 0x16feae54, 0x16febe54, 0x16febf54, 0x17ffff54, 0x17ffff54,
                0x16aaaa54, 0x00000000,
            ],
            colors: [0xffa59958, 0xff47433b, 0xffe8da6e],
        },
    ),
    (
        "sunflower_plains",
        BiomePictogram {
            rows: [
                0x00000000, 0x00000000, 0x00000000, 0x00054000, 0x00165000, 0x001a9000, 0x00165000,
                0x00054000, 0x03c30000, 0x033f0000, 0x03330300, 0x03330330, 0x00c33330, 0x00c3c0c0,
                0x3ffffffc, 0x00000000,
            ],
            colors: [0xfff3c538, 0xff684524, 0xff629c40],
        },
    ),
    (
        "swamp",
        BiomePictogram {
            rows: [
                0x00055000, 0x00155400, 0x01555500, 0x05555540, 0x15555550, 0x15555550, 0x15555550,
                0x05555540, 0x01555540, 0x01155040, 0x010a8040, 0x000a8000, 0x3f0a80fc, 0x000a8000,
                0x000a8000, 0x3ffffffc,
            ],
            colors: [0xff789549, 0xff826348, 0xff58938c],
        },
    ),
    (
        "taiga",
        BiomePictogram {
            rows: [
                0x00000000, 0x00010000, 0x00054000, 0x00054000, 0x00155000, 0x00155000, 0x00555400,
                0x00555400, 0x01555500, 0x05555540, 0x05555540, 0x15555550, 0x000a8000, 0x000a8000,
                0x000a8000, 0x00000000,
            ],
            colors: [0xff487e63, 0xff8c6748, 0xff96b18b],
        },
    ),
    (
        "the_end",
        BiomePictogram {
            rows: [
                0x00000000, 0x003ff000, 0x003ff000, 0x000fc000, 0x000fc000, 0x000fc000, 0x000fc000,
                0x015fd400, 0x055fd500, 0x155fd550, 0x5aaaaaa4, 0x15555550, 0x05555540, 0x01555500,
                0x00555400, 0x00155000,
            ],
            colors: [0xffd7d49e, 0xffefe8ba, 0xffa587cf],
        },
    ),
    (
        "the_void",
        BiomePictogram {
            rows: [
                0x00000000, 0x15000054, 0x10000004, 0x10000004, 0x10000004, 0x00000000, 0x00000000,
                0x00028000, 0x00028000, 0x00000000, 0x00000000, 0x10000004, 0x10000004, 0x10000004,
                0x15400154, 0x00000000,
            ],
            colors: [0xff9b89bd, 0xffd2c3e9, 0xff514867],
        },
    ),
    (
        "warm_ocean",
        BiomePictogram {
            rows: [
                0x00000000, 0x00000000, 0x00000000, 0x00054000, 0x10101004, 0x05400550, 0x00000028,
                0x000002a8, 0x0f0542a8, 0x1f101284, 0x0ffc0690, 0x0ffc0280, 0x003c0280, 0x003c0280,
                0x003c0000, 0x00000000,
            ],
            colors: [0xff48cec5, 0xfff0b27d, 0xffc7649c],
        },
    ),
    (
        "warped_forest",
        BiomePictogram {
            rows: [
                0x00000000, 0x00555400, 0x01555500, 0x05555f50, 0x15555f50, 0x15f55550, 0x15f55554,
                0x15555554, 0x040a8010, 0x040a8010, 0x040a8010, 0x000a8010, 0x000a8010, 0x000a8000,
                0x000a8000, 0x00000000,
            ],
            colors: [0xff37ad9a, 0xff535078, 0xff92d4b1],
        },
    ),
    (
        "windswept_forest",
        BiomePictogram {
            rows: [
                0x3ff00000, 0x00010000, 0x00054ffc, 0x00054000, 0x00155000, 0x00155000, 0x00555400,
                0x00555400, 0x01555500, 0x05555540, 0x05555740, 0x15557fd0, 0x000bfff0, 0x00fffff0,
                0x0ffffffc, 0xffffffff,
            ],
            colors: [0xff628f61, 0xff977757, 0xffb8c4bd],
        },
    ),
    (
        "windswept_gravelly_hills",
        BiomePictogram {
            rows: [
                0x00000000, 0x00010000, 0x00054000, 0x00054000, 0x00155000, 0x00175080, 0x00575480,
                0x005d56a0, 0x015d56a0, 0x01759ff8, 0x05755ffc, 0x05e57ffc, 0x15d57eff, 0x1655efff,
                0x57555554, 0x00000000,
            ],
            colors: [0xff9b9b98, 0xffd1d0c9, 0xff686c70],
        },
    ),
    (
        "windswept_hills",
        BiomePictogram {
            rows: [
                0x00000000, 0x00010000, 0x2aa54000, 0x00054aa8, 0x00155000, 0x00175080, 0x00575480,
                0x005d56a0, 0x015d56a0, 0x01755ff8, 0x05755ffc, 0x05d57ffc, 0x15d57fff, 0x1755ffff,
                0x57555554, 0x00000000,
            ],
            colors: [0xff82907d, 0xffc3c9c5, 0xff607260],
        },
    ),
    (
        "windswept_savanna",
        BiomePictogram {
            rows: [
                0x00000000, 0x15540000, 0x00000000, 0x55415554, 0x00015554, 0x15555554, 0x1555a000,
                0x00028000, 0x00028000, 0x00028c00, 0x0002bf00, 0x0002bfc0, 0x0002ffc0, 0x3ffffff0,
                0x3ffffffc, 0x000fffff,
            ],
            colors: [0xff9a9c52, 0xff826047, 0xffb3aa8a],
        },
    ),
    (
        "wooded_badlands",
        BiomePictogram {
            rows: [
                0x00fffc00, 0x00fffc00, 0x00fffc00, 0x00020000, 0x01565400, 0x01565400, 0x05555400,
                0x05555400, 0x05555554, 0x0ffffffc, 0x15555554, 0x15555554, 0x2aaaaaa8, 0x55555554,
                0x55555554, 0x00000000,
            ],
            colors: [0xffc47b47, 0xffa94f37, 0xff64834b],
        },
    ),
];

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

/// Marker colours a player can choose, as 0xRRGGBB. The first is the default
/// of waypoints saved before colours existed.
pub const WAYPOINT_COLORS: [u32; 8] = [
    0xffd24a, 0xff6b6b, 0x6bd66b, 0x5aa9ff, 0xc78cff, 0xff9f43, 0x4dd0e1, 0xf2f2f2,
];
pub const DEFAULT_WAYPOINT_COLOR: u32 = WAYPOINT_COLORS[0];

/// The pictogram a waypoint's marker shows. Automatic death waypoints use a
/// skull; every other point, including older records, shows its first letter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WaypointIcon {
    #[default]
    Letter,
    Skull,
}

impl WaypointIcon {
    pub fn name(self) -> &'static str {
        match self {
            Self::Letter => "letter",
            Self::Skull => "skull",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "letter" => Some(Self::Letter),
            "skull" => Some(Self::Skull),
            _ => None,
        }
    }
}

/// A point on a map: the dimension it belongs to, its block coordinates, the
/// name, marker colour and icon the player gave it, and the biome sampled
/// there, kept with the point so it survives chunk unloads and reconnects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waypoint {
    pub world: String,
    pub dimension: Dimension,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub name: String,
    /// 0xRRGGBB, never carrying alpha.
    pub color: u32,
    pub icon: WaypointIcon,
    /// Namespaced biome id (for example `minecraft:dappled_forest`), once known.
    pub biome: Option<String>,
    /// Whether this point's marker is drawn in the 3D world (through walls).
    pub show_3d: bool,
}

/// Automatic death waypoints start black so their skull stands out.
pub const DEATH_WAYPOINT_COLOR: u32 = 0x000000;
/// A death waypoint is removed once the player gets this close, in blocks.
pub const DEATH_WAYPOINT_REACH: f64 = 4.0;
const MAX_BIOME_NAME: usize = 128;

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

    /// Keeps a stored colour inside 0xRRGGBB; zero means an older or damaged
    /// entry and falls back to the default marker colour.
    pub fn clean_color(color: u32) -> u32 {
        match color & 0x00ff_ffff {
            0 => DEFAULT_WAYPOINT_COLOR,
            color => color,
        }
    }

    /// Keeps a namespaced biome id printable and bounded. An empty or damaged
    /// value means the biome is still unknown.
    pub fn clean_biome(biome: &str) -> Option<String> {
        let biome: String = biome
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_BIOME_NAME)
            .collect();
        let biome = biome.trim();
        (!biome.is_empty()).then(|| biome.to_owned())
    }

    fn stored_color(&self) -> u32 {
        match self.icon {
            WaypointIcon::Skull => self.color & 0x00ff_ffff,
            WaypointIcon::Letter => Self::clean_color(self.color),
        }
    }

    fn decode(text: &str) -> Option<Self> {
        let (version, rest) = text.split_once('=')?;
        let (fields, version) = match version {
            "waypoint_v5" => (10, 5),
            "waypoint_v4" => (9, 4),
            "waypoint_v3" => (7, 3),
            "waypoint_v2" => (6, 2),
            _ => return None,
        };
        let mut parts = rest.splitn(fields, ':');
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
        let x = parts.next()?.parse().ok()?;
        let y = parts.next()?.parse().ok()?;
        let z = parts.next()?.parse().ok()?;
        let raw_color = if version >= 3 {
            parts
                .next()
                .and_then(|color| u32::from_str_radix(color, 16).ok())
        } else {
            None
        };
        let (icon, biome) = if version >= 4 {
            (
                parts
                    .next()
                    .and_then(WaypointIcon::from_name)
                    .unwrap_or_default(),
                Self::clean_biome(&unescape_name(parts.next().unwrap_or(""))),
            )
        } else {
            (WaypointIcon::Letter, None)
        };
        let color = match (icon, raw_color) {
            (WaypointIcon::Skull, Some(color)) => color & 0x00ff_ffff,
            (_, Some(color)) => Self::clean_color(color),
            (_, None) => DEFAULT_WAYPOINT_COLOR,
        };
        let name = Self::clean_name(&unescape_name(parts.next().unwrap_or("")));
        let show_3d = if version >= 5 {
            parts.next() != Some("0")
        } else {
            true
        };
        Some(Self {
            world,
            dimension,
            x,
            y,
            z,
            color,
            icon,
            biome,
            name,
            show_3d,
        })
    }

    fn encode(&self) -> String {
        format!(
            "waypoint_v5={}:{}:{}:{}:{}:{:06x}:{}:{}:{}:{}\n",
            escape_name(&self.world),
            self.dimension.name(),
            self.x,
            self.y,
            self.z,
            self.stored_color(),
            self.icon.name(),
            escape_name(self.biome.as_deref().unwrap_or("")),
            escape_name(&self.name),
            if self.show_3d { "1" } else { "0" }
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
        // A newly sampled or stored biome wins over the older one, and a point
        // added before the settings file loaded keeps its sample when merging.
        let biome = waypoint.biome.clone().or_else(|| existing.biome.clone());
        let unchanged = existing.name == waypoint.name
            && existing.color == waypoint.color
            && existing.icon == waypoint.icon
            && existing.biome == biome
            && existing.show_3d == waypoint.show_3d;
        existing.name = waypoint.name;
        existing.color = waypoint.color;
        existing.icon = waypoint.icon;
        existing.biome = biome;
        existing.show_3d = waypoint.show_3d;
        return !unchanged;
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

/// Numbers automatic death points: `Death`, `Death 2`, `Death 3`, reusing a
/// freed number after an earlier point was reached and removed.
fn death_waypoint_name(waypoints: &[Waypoint], base: &str) -> String {
    let used = |name: &str| {
        waypoints
            .iter()
            .any(|waypoint| waypoint.icon == WaypointIcon::Skull && waypoint.name == name)
    };
    if !used(base) {
        return base.to_owned();
    }
    let mut number = 2;
    loop {
        let name = format!("{base} {number}");
        if !used(&name) {
            return name;
        }
        number += 1;
    }
}

/// Whether a death waypoint is close enough to pick up after respawning.
pub fn death_waypoint_reached(player: [f64; 3], waypoint: &Waypoint) -> bool {
    if waypoint.icon != WaypointIcon::Skull {
        return false;
    }
    let dx = player[0] - (f64::from(waypoint.x) + 0.5);
    let dy = player[1] - f64::from(waypoint.y);
    let dz = player[2] - (f64::from(waypoint.z) + 0.5);
    dx * dx + dy * dy + dz * dz <= DEATH_WAYPOINT_REACH * DEATH_WAYPOINT_REACH
}

#[derive(Default)]
struct State {
    settings: Settings,
    waypoints: Vec<Waypoint>,
    touched: u32,
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
        let cache_directory = directory.join("cache");
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
            maps: maps::Service::with_directory(Some(cache_directory)),
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
        labels.push(localized_format!(
            "Map RAM: {} MiB…",
            "RAM карты: {} МиБ…",
            "RAM carte : {} Mio…",
            "RAM del mapa: {} MiB…",
            "Karten-RAM: {} MiB…",
            state.settings.cache_ram_mib
        ));
        if self.maps.status().is_some() {
            labels.push(
                tr(
                    "Map cache unavailable: see map status",
                    "Кеш карты недоступен: см. статус на карте",
                    "Cache indisponible : voir la carte",
                    "Caché no disponible: consulta el mapa",
                    "Karten-Cache nicht verfügbar: siehe Karte",
                )
                .into(),
            );
        }

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

    pub fn set_cache_ram_mib(&self, value: u16) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let value = value.clamp(16, 512);
        if state.settings.cache_ram_mib == value {
            return;
        }
        state.settings.cache_ram_mib = value;
        state.touched |= 1 << 17;
        state.revision += 1;
        drop(state);
        if let Some(tx) = self
            .notify
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            let _ = tx.try_send(());
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
        waypoint.color = waypoint.stored_color();
        waypoint.biome = waypoint.biome.as_deref().and_then(Waypoint::clean_biome);
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

    /// Adds the automatic skull waypoint for a death, numbered after the
    /// existing death points of the same world. `biome` is the sample at the
    /// death block when its chunk is loaded.
    pub fn add_death_waypoint(
        &self,
        world: &str,
        dimension: Dimension,
        x: i32,
        y: i32,
        z: i32,
        biome: Option<&str>,
    ) -> bool {
        if !valid_waypoint_world(world) || dimension == Dimension::Player {
            return false;
        }
        let notify = self.notify.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.waypoints.iter().any(|waypoint| {
            waypoint.world == world
                && waypoint.dimension == dimension
                && waypoint.x == x
                && waypoint.z == z
        }) {
            return false;
        }
        let name = death_waypoint_name(
            &state.waypoints,
            tr("Death", "Смерть", "Mort", "Muerte", "Tod"),
        );
        let added = push_waypoint(
            &mut state.waypoints,
            Waypoint {
                world: world.to_owned(),
                dimension,
                x,
                y,
                z,
                name,
                color: DEATH_WAYPOINT_COLOR,
                icon: WaypointIcon::Skull,
                biome: biome.and_then(Waypoint::clean_biome),
                show_3d: true,
            },
        );
        if !added {
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

    /// Stores a freshly sampled biome on an existing point. Only a change
    /// marks the settings dirty, so the file is not rewritten while the player
    /// stays in the same biome.
    pub fn update_waypoint_biome(
        &self,
        world: &str,
        dimension: Dimension,
        x: i32,
        z: i32,
        biome: &str,
    ) -> bool {
        let Some(biome) = Waypoint::clean_biome(biome) else {
            return false;
        };
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
        if waypoint.biome.as_deref() == Some(biome.as_str()) {
            return false;
        }
        waypoint.biome = Some(biome);
        state.revision += 1;
        drop(state);
        if let Some(notify) = notify.as_ref() {
            let _ = notify.try_send(());
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

    pub fn update_waypoint(
        &self,
        world: &str,
        dimension: Dimension,
        x: i32,
        z: i32,
        name: &str,
        color: u32,
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
        let color = match waypoint.icon {
            WaypointIcon::Skull => color & 0x00ff_ffff,
            WaypointIcon::Letter => Waypoint::clean_color(color),
        };
        if waypoint.name == name && waypoint.color == color {
            return false;
        }
        waypoint.name = name;
        waypoint.color = color;
        state.revision += 1;
        drop(state);
        if let Some(notify) = notify.as_ref() {
            let _ = notify.try_send(());
        }
        true
    }

    /// Toggles whether a waypoint's marker is drawn in the 3D world, and
    /// persists the change. False for an unknown point or an unchanged state.
    pub fn set_waypoint_3d(
        &self,
        world: &str,
        dimension: Dimension,
        x: i32,
        z: i32,
        show_3d: bool,
    ) -> bool {
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
        if waypoint.show_3d == show_3d {
            return false;
        }
        waypoint.show_3d = show_3d;
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
            Ok((mut settings, loaded, needs_migration)) => {
                if needs_migration {
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
        text.lines().any(|line| {
            let line = line.trim();
            line.starts_with("waypoint=") || line.starts_with("waypoint_v2=")
        }),
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
        minimap::merge(&mut loaded, edited, 1 << 17);
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

// Shared in-memory terrain repository; retention is temporarily uncapped until the container.
// No game objects cross the worker boundary.
/// Checked single-file terrain storage, owned exclusively by the map worker.
/// Physical unlink/rename and concurrent file readers are deliberately absent.
pub mod container {
    use std::{
        fs::{File, OpenOptions, TryLockError},
        io,
        path::Path,
    };

    #[derive(Debug)]
    pub enum LeaseError {
        Busy,
        Io(io::Error),
    }

    /// Owns exactly one descriptor; never clones it or lends it to readers.
    /// This primitive only opens existing files. Creation and physical deletion
    /// require the future directory/name coordinator and are deliberately absent.
    #[derive(Debug)]
    pub struct ContainerLease {
        file: File,
    }

    impl ContainerLease {
        /// Call on an I/O worker. No bytes are read or changed before ownership.
        /// Unsupported locking is an error, never permission to continue unlocked.
        pub fn open_existing(path: &Path) -> Result<Self, LeaseError> {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(LeaseError::Io)?;
            file.try_lock().map_err(|error| match error {
                TryLockError::WouldBlock => LeaseError::Busy,
                TryLockError::Error(error) => LeaseError::Io(error),
            })?;
            Ok(Self { file })
        }

        pub fn file_len(&self) -> io::Result<u64> {
            self.file.metadata().map(|metadata| metadata.len())
        }
    }

    impl Drop for ContainerLease {
        fn drop(&mut self) {
            // All I/O is synchronous under this owner and has finished now.
            // Explicit release also prevents a transient fork-before-exec copy
            // from extending the lease after the owning worker has stopped.
            let _ = self.file.unlock();
        }
    }

    /// A half-open byte range, validated before arithmetic or allocation.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Extent {
        start: u64,
        end: u64,
    }

    impl Extent {
        pub fn new(start: u64, len: u64) -> Option<Self> {
            let end = start.checked_add(len)?;
            (len != 0).then_some(Self { start, end })
        }

        pub fn start(self) -> u64 {
            self.start
        }

        pub fn byte_len(self) -> u64 {
            self.end - self.start
        }
    }

    /// Rebuild the free-space view from all protected extents: both recoverable
    /// roots, reader pins, and reservations of the in-flight transaction.
    /// Reserved header bytes are never available for reuse. This function does
    /// not persist or trust a free list from disk.
    pub fn free_extents(
        file_len: u64,
        reserved_prefix: u64,
        protected: impl IntoIterator<Item = Extent>,
    ) -> Option<Vec<Extent>> {
        if reserved_prefix > file_len {
            return None;
        }
        let mut protected: Vec<_> = protected.into_iter().collect();
        if protected.iter().any(|extent| extent.end > file_len) {
            return None;
        }
        protected.sort_unstable_by_key(|extent| extent.start);
        let mut cursor = reserved_prefix;
        let mut free = Vec::new();
        for extent in protected {
            if extent.start > cursor {
                free.push(Extent {
                    start: cursor,
                    end: extent.start,
                });
            }
            cursor = cursor.max(extent.end);
        }
        if cursor < file_len {
            free.push(Extent {
                start: cursor,
                end: file_len,
            });
        }
        Some(free)
    }

    /// Reserve one best-fit gap. On failure the free-space view is unchanged;
    /// the caller must grow the file or postpone maintenance, never overwrite
    /// a protected extent. Append/growth is a separate commit responsibility.
    pub fn reserve(free: &mut Vec<Extent>, len: u64) -> Option<Extent> {
        if len == 0 {
            return None;
        }
        let index = free
            .iter()
            .enumerate()
            .filter(|(_, extent)| extent.byte_len() >= len)
            .min_by_key(|(_, extent)| (extent.byte_len(), extent.start))?
            .0;
        let allocation = Extent::new(free[index].start, len)?;
        free[index].start = allocation.end;
        if free[index].start == free[index].end {
            free.remove(index);
        }
        Some(allocation)
    }

    // Version 2: page-aligned allocations, two checked roots, independently checked sorted index pages,
    // checked records. All integer encodings are little endian.
    use std::{
        collections::BTreeMap,
        io::{Read, Seek, SeekFrom, Write},
        path::PathBuf,
    };
    const PAGE: usize = 4096;
    const PREFIX: u64 = (PAGE * 2) as u64;
    const MAX_RECORD: usize = 1024 * 1024;
    pub type RecordKey = [i32; 6];
    #[derive(Clone, Copy, Debug)]
    struct RecordRef {
        offset: u64,
        len: u32,
        checksum: u32,
    }
    #[derive(Clone, Default)]
    struct Root {
        generation: u64,
        entries: BTreeMap<RecordKey, RecordRef>,
        pages: Vec<Extent>,
    }
    pub struct Store {
        lease: ContainerLease,
        world: String,
        roots: [Root; 2],
        current: usize,
        pending: BTreeMap<RecordKey, Vec<u8>>,
        poisoned: bool,
        #[cfg(test)]
        pub(super) write_budget: Option<usize>,
    }
    fn invalid(message: &str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, message)
    }
    fn allocated_len(len: u64) -> io::Result<u64> {
        len.checked_add(PAGE as u64 - 1)
            .map(|n| n / (PAGE as u64) * (PAGE as u64))
            .ok_or_else(|| invalid("allocation length overflow"))
    }
    fn crc(bytes: &[u8]) -> u32 {
        let mut value = !0u32;
        for byte in bytes {
            value ^= u32::from(*byte);
            for _ in 0..8 {
                value = (value >> 1) ^ (0x82f63b78 & 0u32.wrapping_sub(value & 1));
            }
        }
        !value
    }
    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }
    fn u64_at(bytes: &[u8], at: usize) -> u64 {
        u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
    }
    fn put32(bytes: &mut [u8], at: usize, n: u32) {
        bytes[at..at + 4].copy_from_slice(&n.to_le_bytes());
    }
    fn put64(bytes: &mut [u8], at: usize, n: u64) {
        bytes[at..at + 8].copy_from_slice(&n.to_le_bytes());
    }
    fn seal(bytes: &mut [u8]) {
        let end = bytes.len() - 4;
        let c = crc(&bytes[..end]);
        put32(bytes, end, c);
    }
    fn checked(bytes: &[u8]) -> bool {
        crc(&bytes[..bytes.len() - 4]) == u32_at(bytes, bytes.len() - 4)
    }
    fn read_at(file: &mut File, at: u64, bytes: &mut [u8]) -> io::Result<()> {
        file.seek(SeekFrom::Start(at))?;
        file.read_exact(bytes)
    }
    fn write_at(file: &mut File, at: u64, bytes: &[u8]) -> io::Result<()> {
        file.seek(SeekFrom::Start(at))?;
        file.write_all(bytes)
    }
    fn write_with_budget(
        file: &mut File,
        at: u64,
        bytes: &[u8],
        budget: &mut Option<usize>,
    ) -> io::Result<()> {
        if let Some(left) = budget {
            let n = (*left).min(bytes.len());
            write_at(file, at, &bytes[..n])?;
            *left -= n;
            if n < bytes.len() {
                return Err(io::Error::new(
                    io::ErrorKind::StorageFull,
                    "injected short write",
                ));
            }
            Ok(())
        } else {
            write_at(file, at, bytes)
        }
    }
    pub fn world_path(directory: &Path, world: &str) -> PathBuf {
        // The exact identity is also checked in each root: a hash collision never merges worlds.
        let mut a = 0xcbf29ce484222325u64;
        let mut b = 0x84222325cbf29ce4u64;
        for byte in world.bytes() {
            a = (a ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            b = (b ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        directory.join(format!("{a:016x}{b:016x}.mwwr"))
    }
    impl Store {
        pub fn open(directory: &Path, world: &str) -> Result<Self, LeaseError> {
            if world.is_empty() || world.len() > 2048 {
                return Err(LeaseError::Io(invalid("invalid world identity")));
            }
            std::fs::create_dir_all(directory).map_err(LeaseError::Io)?;
            // No participant renames/unlinks these files. A create race converges
            // on one inode; initialization happens only after try_lock succeeds.
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(world_path(directory, world))
                .map_err(LeaseError::Io)?;
            file.try_lock().map_err(|e| match e {
                TryLockError::WouldBlock => LeaseError::Busy,
                TryLockError::Error(e) => LeaseError::Io(e),
            })?;
            let mut store = Self {
                lease: ContainerLease { file },
                world: world.into(),
                roots: Default::default(),
                current: 0,
                pending: BTreeMap::new(),
                poisoned: false,
                #[cfg(test)]
                write_budget: None,
            };
            store.load().map_err(LeaseError::Io)?;
            Ok(store)
        }
        fn load(&mut self) -> io::Result<()> {
            let len = self.lease.file.metadata()?.len();
            if len == 0 {
                self.lease.file.set_len(PREFIX)?;
                self.commit_inner()?;
                return Ok(());
            }
            if len < PREFIX {
                return Err(invalid("truncated container header"));
            }
            let mut valid = [false; 2];
            for (slot, validity) in valid.iter_mut().enumerate() {
                let mut header = vec![0; PAGE];
                read_at(&mut self.lease.file, (slot * PAGE) as u64, &mut header)?;
                if &header[..8] == b"MWWRC002" && u32_at(&header, 8) != 2 {
                    return Err(invalid("unsupported container version"));
                }
                if &header[..8] != b"MWWRC002" || !checked(&header) {
                    continue;
                }
                let world_len = u32_at(&header, 40) as usize;
                if world_len > 2048 || header[48..48 + world_len] != *self.world.as_bytes() {
                    return Err(invalid("world identity mismatch"));
                }
                if let Ok(root) = self.read_root(&header, len) {
                    self.roots[slot] = root;
                    *validity = true;
                }
            }
            self.current = match valid {
                [false, false] if len == PREFIX => {
                    let mut bytes = vec![0; PREFIX as usize];
                    read_at(&mut self.lease.file, 0, &mut bytes)?;
                    if bytes.iter().any(|b| *b != 0) {
                        return Err(invalid("no recoverable container root"));
                    }
                    self.commit_inner()?;
                    return Ok(());
                }
                [false, false] => return Err(invalid("no recoverable container root")),
                [true, false] => 0,
                [false, true] => 1,
                [true, true] => usize::from(self.roots[1].generation > self.roots[0].generation),
            };
            Ok(())
        }
        fn read_root(&mut self, header: &[u8], file_len: u64) -> io::Result<Root> {
            let generation = u64_at(header, 16);
            if generation == 0 {
                return Err(invalid("zero generation"));
            }
            let mut root = Root {
                generation,
                ..Root::default()
            };
            let mut next = u64_at(header, 24);
            let count = u64_at(header, 32);
            if count > file_len / 40 {
                return Err(invalid("invalid index count"));
            }
            let mut previous = None;
            while next != 0 {
                if root.pages.len() as u64 > count / 100 + 1
                    || next < PREFIX
                    || !next.is_multiple_of(PAGE as u64)
                    || next
                        .checked_add(PAGE as u64)
                        .is_none_or(|end| end > file_len)
                {
                    return Err(invalid("invalid index extent"));
                }
                let mut page = vec![0; PAGE];
                read_at(&mut self.lease.file, next, &mut page)?;
                if &page[..8] != b"MWWRIDX1" || !checked(&page) || u64_at(&page, 8) != generation {
                    return Err(invalid("invalid index page"));
                }
                root.pages.push(Extent::new(next, PAGE as u64).unwrap());
                let n = u32_at(&page, 24) as usize;
                if n == 0 || n > 100 {
                    return Err(invalid("invalid index page size"));
                }
                for i in 0..n {
                    let at = 32 + i * 40;
                    let key = std::array::from_fn(|j| u32_at(&page, at + j * 4) as i32);
                    if previous.is_some_and(|old| old >= key) {
                        return Err(invalid("unsorted index"));
                    }
                    previous = Some(key);
                    let value = RecordRef {
                        offset: u64_at(&page, at + 24),
                        len: u32_at(&page, at + 32),
                        checksum: u32_at(&page, at + 36),
                    };
                    if value.offset < PREFIX
                        || !value.offset.is_multiple_of(PAGE as u64)
                        || value.len as usize > MAX_RECORD
                        || value.len == 0
                        || value
                            .offset
                            .checked_add(allocated_len(u64::from(value.len))?)
                            .is_none_or(|end| end > file_len)
                    {
                        return Err(invalid("invalid record extent"));
                    }
                    root.entries.insert(key, value);
                }
                next = u64_at(&page, 16);
            }
            if root.entries.len() as u64 != count {
                return Err(invalid("incomplete index"));
            }
            // Reject overlapping records/pages, including self-overlapping indexes.
            let mut extents = root.pages.clone();
            extents.extend(
                root.entries.values().map(|r| {
                    Extent::new(r.offset, allocated_len(u64::from(r.len)).unwrap()).unwrap()
                }),
            );
            extents.sort_unstable_by_key(|e| e.start);
            if extents.windows(2).any(|e| e[0].end > e[1].start) {
                return Err(invalid("overlapping extents"));
            }
            Ok(root)
        }
        pub fn keys(&self) -> impl Iterator<Item = RecordKey> + '_ {
            self.roots[self.current].entries.keys().copied().chain(
                self.pending
                    .keys()
                    .filter(|k| !self.roots[self.current].entries.contains_key(*k))
                    .copied(),
            )
        }
        pub fn read(&mut self, key: RecordKey) -> io::Result<Option<Vec<u8>>> {
            if let Some(data) = self.pending.get(&key) {
                return Ok(Some(data.clone()));
            }
            let Some(record) = self.roots[self.current].entries.get(&key).copied() else {
                return Ok(None);
            };
            let mut bytes = vec![0; record.len as usize];
            read_at(&mut self.lease.file, record.offset, &mut bytes)?;
            if crc(&bytes) != record.checksum {
                return Err(invalid("record checksum mismatch"));
            }
            Ok(Some(bytes))
        }
        pub fn put(&mut self, key: RecordKey, bytes: Vec<u8>) -> io::Result<()> {
            if self.poisoned {
                return Err(invalid("writer needs recovery after an I/O failure"));
            }
            if bytes.is_empty() || bytes.len() > MAX_RECORD {
                return Err(invalid("invalid record length"));
            }
            self.pending.insert(key, bytes);
            Ok(())
        }
        pub fn pending_bytes(&self) -> usize {
            self.pending.values().map(Vec::len).sum()
        }
        pub fn dirty(&self) -> bool {
            !self.pending.is_empty()
        }
        pub fn commit(&mut self) -> io::Result<()> {
            if self.poisoned {
                return Err(invalid("writer needs recovery after an I/O failure"));
            }
            if !self.dirty() {
                return Ok(());
            }
            let result = self.commit_inner();
            if result.is_err() {
                self.poisoned = true;
            }
            result
        }
        fn commit_inner(&mut self) -> io::Result<()> {
            #[cfg(test)]
            let mut write_budget = self.write_budget.take();
            #[cfg(not(test))]
            let mut write_budget = None;

            let generation = self.roots[self.current]
                .generation
                .checked_add(1)
                .ok_or_else(|| invalid("generation overflow"))?;
            let file_len = allocated_len(self.lease.file.metadata()?.len())?;
            let protected = self.roots.iter().flat_map(|root| {
                root.pages
                    .iter()
                    .copied()
                    .chain(root.entries.values().map(|r| {
                        Extent::new(r.offset, allocated_len(u64::from(r.len)).unwrap()).unwrap()
                    }))
            });
            let mut free = free_extents(file_len, PREFIX, protected)
                .ok_or_else(|| invalid("bad free-space view"))?;
            let mut end = file_len;
            let mut allocate = |len: u64| -> io::Result<Extent> {
                if let Some(e) = reserve(&mut free, len) {
                    return Ok(e);
                }
                let e = Extent::new(end, len).ok_or_else(|| invalid("file offset overflow"))?;
                end = e.end;
                Ok(e)
            };
            let mut root = Root {
                generation,
                entries: self.roots[self.current].entries.clone(),
                pages: Vec::new(),
            };
            for (&key, bytes) in &self.pending {
                let extent = allocate(allocated_len(bytes.len() as u64)?)?;
                write_with_budget(&mut self.lease.file, extent.start, bytes, &mut write_budget)?;
                root.entries.insert(
                    key,
                    RecordRef {
                        offset: extent.start,
                        len: bytes.len() as u32,
                        checksum: crc(bytes),
                    },
                );
            }
            let entries: Vec<_> = root.entries.iter().collect();
            for _ in entries.chunks(100) {
                root.pages.push(allocate(PAGE as u64)?);
            }
            for (i, chunk) in entries.chunks(100).enumerate() {
                let mut page = vec![0; PAGE];
                page[..8].copy_from_slice(b"MWWRIDX1");
                put64(&mut page, 8, generation);
                put64(&mut page, 16, root.pages.get(i + 1).map_or(0, |e| e.start));
                put32(&mut page, 24, chunk.len() as u32);
                for (j, (key, r)) in chunk.iter().enumerate() {
                    let at = 32 + j * 40;
                    for (k, n) in key.iter().enumerate() {
                        put32(&mut page, at + k * 4, *n as u32);
                    }
                    put64(&mut page, at + 24, r.offset);
                    put32(&mut page, at + 32, r.len);
                    put32(&mut page, at + 36, r.checksum);
                }
                seal(&mut page);
                write_with_budget(
                    &mut self.lease.file,
                    root.pages[i].start,
                    &page,
                    &mut write_budget,
                )?;
            }
            self.lease.file.set_len(end)?;
            self.lease.file.sync_all()?;
            let slot = 1 - self.current;
            let mut header = vec![0; PAGE];
            header[..8].copy_from_slice(b"MWWRC002");
            put32(&mut header, 8, 2);
            put64(&mut header, 16, generation);
            put64(&mut header, 24, root.pages.first().map_or(0, |e| e.start));
            put64(&mut header, 32, root.entries.len() as u64);
            put32(&mut header, 40, self.world.len() as u32);
            header[48..48 + self.world.len()].copy_from_slice(self.world.as_bytes());
            seal(&mut header);
            write_with_budget(
                &mut self.lease.file,
                (slot * PAGE) as u64,
                &header,
                &mut write_budget,
            )?;
            self.lease.file.sync_all()?;
            self.roots[slot] = root;
            self.current = slot;
            self.pending.clear();
            Ok(())
        }
    }
    #[cfg(test)]
    mod tests {
        #[test]
        fn store_creation_child() {
            let Some(dir) = std::env::var_os("MWWR_STORE_CREATE_DIR") else {
                return;
            };
            let mut input = std::io::stdin();
            let mut go = [0];
            input.read_exact(&mut go).unwrap();
            let owned = Store::open(Path::new(&dir), "server:creation-race");
            match &owned {
                Ok(_) => println!("MWWR_CREATE_OWNED"),
                Err(LeaseError::Busy) => println!("MWWR_CREATE_BUSY"),
                Err(e) => panic!("{e:?}"),
            }
            std::io::stdout().flush().unwrap();
            input.read_exact(&mut go).unwrap(); // Hold ownership until the parent finishes observing both.
            drop(owned);
        }
        #[test]
        fn simultaneous_process_creation_initializes_only_one_file() {
            let dir = TestDir::new("create-race");
            let cache = dir.0.join("cache");
            let (sender, receiver) = std::sync::mpsc::channel();
            let mut children = Vec::new();
            let mut readers = Vec::new();
            for _ in 0..2 {
                let name = format!(
                    "{}::store_creation_child",
                    module_path!().split_once("::").unwrap().1
                );
                let mut child = ChildGuard(
                    Command::new(std::env::current_exe().unwrap())
                        .args(["--exact", &name, "--nocapture"])
                        .env("MWWR_STORE_CREATE_DIR", &cache)
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .spawn()
                        .unwrap(),
                );
                let stdout = child.0.stdout.take().unwrap();
                let sender = sender.clone();
                readers.push(std::thread::spawn(move || {
                    for line in BufReader::new(stdout).lines() {
                        let line = line.unwrap();
                        if line.contains("MWWR_CREATE_") {
                            sender.send(line).unwrap();
                        }
                    }
                }));
                children.push(child);
            }
            for child in &mut children {
                child.0.stdin.as_mut().unwrap().write_all(b"1").unwrap();
            }
            let lines: Vec<_> = (0..2)
                .map(|_| {
                    receiver
                        .recv_timeout(std::time::Duration::from_secs(10))
                        .unwrap()
                })
                .collect();
            assert_eq!(lines.iter().filter(|s| s.contains("OWNED")).count(), 1);
            assert_eq!(lines.iter().filter(|s| s.contains("BUSY")).count(), 1);
            for child in &mut children {
                child.0.stdin.as_mut().unwrap().write_all(b"2").unwrap();
                assert!(child.0.wait().unwrap().success());
            }
            for reader in readers {
                reader.join().unwrap();
            }
            let store = Store::open(&cache, "server:creation-race").unwrap();
            assert_eq!(store.keys().count(), 0);
            assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 1);
        }
        struct TestDir(std::path::PathBuf);
        impl TestDir {
            fn new(label: &str) -> Self {
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let dir = std::env::temp_dir().join(format!(
                    "mwwr-{label}-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ));
                std::fs::create_dir(&dir).unwrap();
                Self(dir)
            }
        }
        impl Drop for TestDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        #[test]
        fn records_reopen_across_pages_and_worlds_and_reuse_space() {
            let dir = TestDir::new("index");
            let key = [1, 0, 0, -1, 2, 0];
            let mut store = Store::open(&dir.0, "server:example:25565").unwrap();
            for x in 0..205 {
                store.put([1, 0, 0, x, 0, 0], vec![x as u8; 100]).unwrap();
            }
            store.put(key, b"first".to_vec()).unwrap();
            store.commit().unwrap();
            drop(store);
            let mut store = Store::open(&dir.0, "server:example:25565").unwrap();
            assert_eq!(store.keys().count(), 206);
            assert_eq!(store.read(key).unwrap().unwrap(), b"first");
            for x in 0..205 {
                assert_eq!(
                    store.read([1, 0, 0, x, 0, 0]).unwrap().unwrap(),
                    vec![x as u8; 100]
                );
            }
            let mut other = Store::open(&dir.0, "save:example:25565").unwrap();
            assert!(other.read(key).unwrap().is_none());
            drop(other);
            for i in 0..15 {
                store.put(key, vec![i; 1024]).unwrap();
                store.commit().unwrap();
            }
            let size = store.lease.file.metadata().unwrap().len();
            for i in 0..30 {
                store.put(key, vec![i; 1024]).unwrap();
                store.commit().unwrap();
            }
            assert!(store.lease.file.metadata().unwrap().len() <= size + 8192);
            assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 2);
        }
        #[test]
        fn every_short_write_phase_keeps_an_old_root_and_poisoned_writer_stops() {
            let dir = TestDir::new("fault");
            let key = [1, 0, 0, 0, 0, 0];
            let mut base = Store::open(&dir.0, "server:fault").unwrap();
            base.put(key, b"old".to_vec()).unwrap();
            base.commit().unwrap();
            drop(base);
            let path = world_path(&dir.0, "server:fault");
            let original = std::fs::read(&path).unwrap();
            for budget in [
                0, 1, 63, 64, 65, 100, 4095, 4096, 4159, 4160, 4161, 8191, 8255,
            ] {
                std::fs::write(&path, &original).unwrap();
                let mut store = Store::open(&dir.0, "server:fault").unwrap();
                store.put(key, vec![7; 64]).unwrap();
                store.write_budget = Some(budget);
                assert!(store.commit().is_err(), "budget {budget}");
                assert!(store.put(key, b"bad".to_vec()).is_err());
                assert!(store.dirty());
                drop(store);
                let mut recovered = Store::open(&dir.0, "server:fault").unwrap();
                assert_eq!(
                    recovered.read(key).unwrap().unwrap(),
                    b"old",
                    "budget {budget}"
                );
                // A failed append can leave an unaligned orphan tail. Recovery
                // must still allocate full pages without touching the old root.
                recovered.put(key, b"recovered".to_vec()).unwrap();
                recovered.commit().unwrap();
                assert_eq!(recovered.read(key).unwrap().unwrap(), b"recovered");
                assert!(
                    recovered
                        .lease
                        .file
                        .metadata()
                        .unwrap()
                        .len()
                        .is_multiple_of(PAGE as u64)
                );
            }
        }
        #[test]
        fn bad_new_index_recovers_previous_root_unknown_schema_never_overwrites() {
            let dir = TestDir::new("recovery");
            let key = [1, 0, 0, 0, 0, 0];
            let mut store = Store::open(&dir.0, "server:recovery").unwrap();
            store.put(key, b"old".to_vec()).unwrap();
            store.commit().unwrap();
            store.put(key, b"new".to_vec()).unwrap();
            store.commit().unwrap();
            let page = store.roots[store.current].pages[0].start;
            write_at(&mut store.lease.file, page, b"corrupt!").unwrap();
            drop(store);
            let mut recovered = Store::open(&dir.0, "server:recovery").unwrap();
            assert_eq!(recovered.read(key).unwrap().unwrap(), b"old");
            let mut header = vec![0; PAGE];
            header[..8].copy_from_slice(b"MWWRC002");
            put32(&mut header, 8, 3);
            seal(&mut header);
            write_at(&mut recovered.lease.file, 0, &header).unwrap();
            drop(recovered);
            let path = world_path(&dir.0, "server:recovery");
            let before = std::fs::read(&path).unwrap();
            assert!(matches!(
                Store::open(&dir.0, "server:recovery"),
                Err(LeaseError::Io(_))
            ));
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
        #[test]
        fn corrupt_record_is_isolated_and_world_identity_is_checked() {
            let dir = TestDir::new("checksum");
            let key = [1, 0, 0, 0, 0, 0];
            let other = [2, 1, 48, 0, 0, 0];
            let mut store = Store::open(&dir.0, "server:checksum").unwrap();
            store.put(key, b"terrain".to_vec()).unwrap();
            store.put(other, b"nether".to_vec()).unwrap();
            store.commit().unwrap();
            let offset = store.roots[store.current].entries[&key].offset;
            write_at(&mut store.lease.file, offset, b"!").unwrap();
            assert!(store.read(key).is_err());
            assert_eq!(store.read(other).unwrap().unwrap(), b"nether");
            drop(store);
            let wrong = world_path(&dir.0, "server:other");
            std::fs::copy(world_path(&dir.0, "server:checksum"), &wrong).unwrap();
            let before = std::fs::read(&wrong).unwrap();
            assert!(Store::open(&dir.0, "server:other").is_err());
            assert_eq!(std::fs::read(&wrong).unwrap(), before);
        }

        use super::*;
        use std::{
            io::{BufRead, BufReader, Write},
            process::{Child, Command, Stdio},
        };

        fn extent(start: u64, len: u64) -> Extent {
            Extent::new(start, len).unwrap()
        }

        #[test]
        fn both_roots_readers_and_reservations_protect_space() {
            let protected = [
                extent(20, 20),
                extent(30, 30),
                extent(80, 10),
                extent(90, 5),
            ];
            let mut free = free_extents(100, 16, protected).unwrap();
            assert_eq!(free, [extent(16, 4), extent(60, 20), extent(95, 5)]);
            assert_eq!(reserve(&mut free, 5), Some(extent(95, 5)));
            assert_eq!(reserve(&mut free, 18), Some(extent(60, 18)));
            let before = free.clone();
            assert_eq!(reserve(&mut free, 6), None);
            assert_eq!(reserve(&mut free, 0), None);
            assert_eq!(free, before);
            // Retire the old root, but its overlapping reader pin still wins.
            assert_eq!(
                free_extents(100, 16, [extent(30, 30)]).unwrap(),
                [extent(16, 14), extent(60, 40)]
            );
        }

        #[test]
        fn invalid_ranges_fail_closed() {
            assert_eq!(Extent::new(u64::MAX, 1), None);
            assert_eq!(Extent::new(0, 0), None);
            assert_eq!(free_extents(10, 11, []), None);
            assert_eq!(free_extents(10, 0, [extent(9, 2)]), None);
            assert_eq!(free_extents(10, 10, []), Some(vec![]));
            assert_eq!(free_extents(10, 5, [extent(0, 10)]), Some(vec![]));
        }

        // The child is the same test executable, so these are real OS processes.
        #[test]
        fn lease_child() {
            let Some(path) = std::env::var_os("MWWR_LEASE_TEST_PATH") else {
                return;
            };
            if std::env::var_os("MWWR_LEASE_TEST_BUSY").is_some() {
                assert!(matches!(
                    ContainerLease::open_existing(Path::new(&path)),
                    Err(LeaseError::Busy)
                ));
                return;
            }
            let _lease = ContainerLease::open_existing(Path::new(&path)).unwrap();
            println!("MWWR_LEASE_READY");
            std::io::stdout().flush().unwrap();
            loop {
                std::thread::park();
            }
        }

        struct ChildGuard(Child);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        fn child(path: &Path) -> Command {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    &format!(
                        "{}::lease_child",
                        module_path!().split_once("::").unwrap().1
                    ),
                    "--nocapture",
                ])
                .env("MWWR_LEASE_TEST_PATH", path);
            command
        }

        #[test]
        fn processes_busy_crash_release_and_independent_worlds() {
            let dir = std::env::temp_dir().join(format!("mwwr-lease-{}", std::process::id()));
            std::fs::create_dir(&dir).unwrap();
            let path = dir.join("world.mwwr");
            let other = dir.join("other.mwwr");
            std::fs::write(&path, b"unchanged sentinel").unwrap();
            std::fs::write(&other, b"other world").unwrap();
            let lease = ContainerLease::open_existing(&path).unwrap();
            assert_eq!(lease.file_len().unwrap(), 18);
            assert!(
                child(&path)
                    .env("MWWR_LEASE_TEST_BUSY", "1")
                    .status()
                    .unwrap()
                    .success()
            );
            assert!(matches!(
                ContainerLease::open_existing(&path),
                Err(LeaseError::Busy)
            ));
            let other_lease = ContainerLease::open_existing(&other).unwrap();
            drop(other_lease);
            drop(lease);
            let mut owner = ChildGuard(child(&path).stdout(Stdio::piped()).spawn().unwrap());
            let stdout = owner.0.stdout.take().unwrap();
            let (sender, receiver) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    if line.unwrap().contains("MWWR_LEASE_READY") {
                        let _ = sender.send(());
                        break;
                    }
                }
            });
            receiver
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
            assert!(matches!(
                ContainerLease::open_existing(&path),
                Err(LeaseError::Busy)
            ));
            drop(owner); // Simulate an abrupt owner death, without unlock/flush.
            reader.join().unwrap();
            let recovered = ContainerLease::open_existing(&path).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), b"unchanged sentinel");
            drop(recovered);
            assert!(matches!(
                ContainerLease::open_existing(&dir.join("missing")),
                Err(LeaseError::Io(_))
            ));
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}

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

    /// Whether a block's map colour needs the biome's grass tint.
    pub fn needs_grass_tint(name: &str) -> bool {
        matches!(
            name,
            "minecraft:grass_block" | "minecraft:grass" | "minecraft:lily_pad"
        )
    }

    /// Whether a block's map colour needs the biome's foliage tint.
    pub fn needs_foliage_tint(name: &str) -> bool {
        matches!(
            name,
            "minecraft:oak_leaves"
                | "minecraft:jungle_leaves"
                | "minecraft:acacia_leaves"
                | "minecraft:dark_oak_leaves"
                | "minecraft:mangrove_leaves"
                | "minecraft:vine"
        )
    }

    /// The map colour of a block after its biome tint: a grass block's top is
    /// drawn with the biome's grass colour, foliage leaves and vines with its
    /// foliage colour, and birch and spruce keep the fixed shades the world
    /// gives them. Unknown biomes keep `base`.
    pub fn block_map_color(name: &str, base: u32, grass: Option<u32>, foliage: Option<u32>) -> u32 {
        match name {
            "minecraft:grass_block" | "minecraft:grass" | "minecraft:lily_pad" => {
                grass.map_or(base, |grass| grass & 0x00ff_ffff)
            }
            "minecraft:birch_leaves" => 0x80a755,
            "minecraft:spruce_leaves" => 0x619961,
            "minecraft:oak_leaves"
            | "minecraft:jungle_leaves"
            | "minecraft:acacia_leaves"
            | "minecraft:dark_oak_leaves"
            | "minecraft:mangrove_leaves"
            | "minecraft:vine" => foliage.map_or(base, |foliage| foliage & 0x00ff_ffff),
            _ => base,
        }
    }

    /// A shallow seabed keeps most of its colour; deep water darkens it in
    /// steps instead of looking one block deep everywhere.
    fn water_alpha(depth: u32) -> u8 {
        (70 + depth.saturating_mul(10)).min(190) as u8
    }

    #[cfg(test)]
    pub fn sample(
        block: impl FnMut(i32, i32, i32) -> Block,
        top: impl Fn(i32, i32) -> i32,
        chunk: [i32; 2],
        minimum: i32,
        layer: Layer,
    ) -> Tile {
        sample_with_biomes(block, top, chunk, minimum, layer, |_| None)
    }

    /// Resolve the biome at the visible top of each column, including water,
    /// from the same snapshot as its terrain, before the game unloads it.
    pub fn sample_with_biomes(
        mut block: impl FnMut(i32, i32, i32) -> Block,
        top: impl Fn(i32, i32) -> i32,
        chunk: [i32; 2],
        minimum: i32,
        layer: Layer,
        mut biome: impl FnMut([i32; 3]) -> Option<Arc<str>>,
    ) -> Tile {
        let mut tile = Tile {
            pixels: vec![0; 16 * 16 * 4],
            heights: [minimum; 256],
            walls: [false; 256],
            portals: Vec::new(),
            biomes: vec![None; 256],
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
                tile.biomes[i] = biome([wx, top_y.unwrap_or(visible_y), wz]);
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
    }
    impl View {
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
        detailed_data: BTreeMap<Key, Arc<Tile>>,
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
        pub fn detailed_biome(
            &self,
            dimension: Dimension,
            layer: Layer,
            at: [i32; 2],
        ) -> Option<Arc<str>> {
            let key = (dimension, layer, at[0].div_euclid(16), at[1].div_euclid(16));
            let tile = self.detailed_data.get(&key)?;
            let cell = (at[1].rem_euclid(16) * 16 + at[0].rem_euclid(16)) as usize;
            tile.biomes.get(cell).cloned().flatten()
        }
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
        pub pixels: Vec<u8>,
        pub height: usize,
        pub chunks: Vec<[i32; 2]>,
        pub coarse: Vec<LodDraw>,
        pub region: [i64; 4],
        pub width: usize,
    }
    impl TerrainAtlas {
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
                let size = 16;
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
        terrain: Arc<TerrainAtlas>,
    }
    pub struct Frame {
        pub raster_source: Option<Arc<Frame>>,
        pub serial: u64,
        pub view: View,
        pub pixels: Vec<u8>,
        pub known: usize,
        pub atlas: Option<Arc<TerrainAtlas>>,
    }

    /// Reproject an axis-aligned raster into the current world-map camera.
    pub fn raster_quad(source: &View, view: &View, rect: [f32; 4]) -> Option<([f32; 4], [f32; 4])> {
        let mut src = [0.0; 4];
        let mut dst = [0.0; 4];
        for axis in 0..2 {
            let origin = source.center[axis] - source.span[axis] / 2.0;
            let low = view.center[axis] - view.span[axis] / 2.0;
            let start = origin.max(low);
            let end = (origin + source.span[axis]).min(low + view.span[axis]);
            if end <= start {
                return None;
            }
            let pixels = [source.width, source.height][axis] as f64;
            src[axis] = ((start - origin) / source.span[axis] * pixels) as f32;
            src[axis + 2] = ((end - start) / source.span[axis] * pixels) as f32;
            dst[axis] = rect[axis] + ((start - low) / view.span[axis]) as f32 * rect[axis + 2];
            dst[axis + 2] = ((end - start) / view.span[axis]) as f32 * rect[axis + 2];
        }
        Some((src, dst))
    }

    /// Publish geometry only after its exact immutable texture is uploaded.
    /// Keeping the previous Arc alive also makes pointer identity safe: equal
    /// dimensions or serials alone do not identify a texture's slot layout.
    pub fn present_frame(
        displayed: &mut Option<Arc<Frame>>,
        next: Arc<Frame>,
        upload: impl FnOnce(u32, u32, &[u8]) -> bool,
    ) -> bool {
        let same_texture =
            displayed
                .as_ref()
                .is_some_and(|previous| match (&previous.atlas, &next.atlas) {
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                    (None, None) => Arc::ptr_eq(
                        previous.raster_source.as_ref().unwrap_or(previous),
                        next.raster_source.as_ref().unwrap_or(&next),
                    ),
                    _ => false,
                });
        if !same_texture {
            let (width, height, pixels) = match &next.atlas {
                Some(atlas) => (atlas.width, atlas.height, &atlas.pixels),
                None => {
                    let source = next.raster_source.as_ref().unwrap_or(&next);
                    (source.view.width, source.view.height, &source.pixels)
                }
            };
            if !upload(width as u32, height as u32, pixels) {
                return false;
            }
        }
        *displayed = Some(next);
        true
    }
    use super::container::{self, RecordKey, Store};
    use std::{
        cell::RefCell,
        io,
        path::Path,
        time::{Duration, Instant},
    };

    fn record_key(d: Dimension, layer: Layer, x: i32, z: i32) -> RecordKey {
        [
            match d {
                Dimension::Player => 0,
                Dimension::Overworld => 1,
                Dimension::Nether => 2,
                Dimension::End => 3,
            },
            i32::from(matches!(layer, Layer::Below(_))),
            match layer {
                Layer::Surface => 0,
                Layer::Below(y) => y,
            },
            x,
            z,
            0,
        ]
    }
    fn lod_record(d: Dimension, key: LodKey) -> RecordKey {
        [
            match d {
                Dimension::Player => 0,
                Dimension::Overworld => 1,
                Dimension::Nether => 2,
                Dimension::End => 3,
            },
            2,
            0,
            key.x,
            key.z,
            i32::from(key.level),
        ]
    }
    fn record_dimension(key: RecordKey) -> Option<Dimension> {
        match key[0] {
            1 => Some(Dimension::Overworld),
            2 => Some(Dimension::Nether),
            3 => Some(Dimension::End),
            _ => None,
        }
    }
    fn record_layer(key: RecordKey) -> Option<Layer> {
        match key[1] {
            0 if key[2] == 0 => Some(Layer::Surface),
            1 => Some(Layer::Below(key[2])),
            _ => None,
        }
    }
    const MAX_TILE_PORTALS: usize = 256 * 3;
    fn tile_bytes(tile: &Tile) -> usize {
        1024 + 256
            + std::mem::size_of::<Tile>()
            + tile.pixels.capacity()
            + tile.biomes.capacity() * std::mem::size_of::<Option<Arc<str>>>()
            + tile
                .biomes
                .iter()
                .flatten()
                .map(|s| s.len() + 32)
                .sum::<usize>()
            + tile.portals.capacity() * 32
    }
    fn tile_encode(tile: &Tile) -> io::Result<Vec<u8>> {
        if tile.pixels.len() != 1024
            || tile.biomes.len() > 256
            || tile.portals.len() > MAX_TILE_PORTALS
        {
            return Err(io::Error::other("invalid terrain tile"));
        }
        let mut bytes = Vec::with_capacity(4096);
        bytes.extend_from_slice(b"TIL1");
        bytes.extend_from_slice(&0u32.to_le_bytes()); // Per-record codec: 0 = raw.
        bytes.extend_from_slice(&tile.pixels);
        for n in tile.heights {
            bytes.extend_from_slice(&n.to_le_bytes());
        }
        bytes.extend(tile.walls.iter().map(|v| u8::from(*v)));
        let mut palette = BTreeMap::<&str, u16>::new();
        for name in tile.biomes.iter().flatten() {
            if name.is_empty() || name.len() > 256 {
                return Err(io::Error::other("invalid biome name"));
            }
            let next = palette.len() as u16 + 1;
            palette.entry(name.as_ref()).or_insert(next);
        }
        bytes.extend_from_slice(&(palette.len() as u16).to_le_bytes());
        let mut ordered: Vec<_> = palette.iter().collect();
        ordered.sort_unstable_by_key(|(_, id)| **id);
        for (name, _) in ordered {
            bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
            bytes.extend_from_slice(name.as_bytes());
        }
        for i in 0..256 {
            let id = tile
                .biomes
                .get(i)
                .and_then(Option::as_ref)
                .and_then(|name| palette.get(name.as_ref()))
                .copied()
                .unwrap_or(0);
            bytes.extend_from_slice(&id.to_le_bytes());
        }
        bytes.extend_from_slice(&(tile.portals.len() as u16).to_le_bytes());
        for (at, kind) in &tile.portals {
            for n in at {
                bytes.extend_from_slice(&n.to_le_bytes());
            }
            bytes.push(match kind {
                Portal::Nether => 0,
                Portal::End => 1,
                Portal::Gateway => 2,
            });
        }
        Ok(bytes)
    }
    fn tile_decode(bytes: &[u8]) -> io::Result<Tile> {
        fn bad() -> io::Error {
            io::Error::new(io::ErrorKind::InvalidData, "invalid terrain record")
        }
        fn take<'a>(bytes: &mut &'a [u8], n: usize) -> io::Result<&'a [u8]> {
            if bytes.len() < n {
                return Err(bad());
            }
            let (a, b) = bytes.split_at(n);
            *bytes = b;
            Ok(a)
        }
        let mut bytes = bytes;
        if take(&mut bytes, 4)? != b"TIL1" || take(&mut bytes, 4)? != 0u32.to_le_bytes() {
            return Err(bad());
        }
        let mut tile = Tile {
            pixels: take(&mut bytes, 1024)?.to_vec(),
            ..Tile::default()
        };
        for n in &mut tile.heights {
            *n = i32::from_le_bytes(take(&mut bytes, 4)?.try_into().unwrap());
        }
        for wall in &mut tile.walls {
            *wall = match take(&mut bytes, 1)?[0] {
                0 => false,
                1 => true,
                _ => return Err(bad()),
            };
        }
        let count = u16::from_le_bytes(take(&mut bytes, 2)?.try_into().unwrap()) as usize;
        if count > 256 {
            return Err(bad());
        }
        let mut palette = vec![None];
        for _ in 0..count {
            let len = u16::from_le_bytes(take(&mut bytes, 2)?.try_into().unwrap()) as usize;
            if len == 0 || len > 256 {
                return Err(bad());
            }
            let name = std::str::from_utf8(take(&mut bytes, len)?).map_err(|_| bad())?;
            palette.push(Some(Arc::from(name)));
        }
        for _ in 0..256 {
            let id = u16::from_le_bytes(take(&mut bytes, 2)?.try_into().unwrap()) as usize;
            tile.biomes.push(palette.get(id).ok_or_else(bad)?.clone());
        }
        let count = u16::from_le_bytes(take(&mut bytes, 2)?.try_into().unwrap()) as usize;
        if count > MAX_TILE_PORTALS {
            return Err(bad());
        }
        for _ in 0..count {
            let mut at = [0; 3];
            for n in &mut at {
                *n = i32::from_le_bytes(take(&mut bytes, 4)?.try_into().unwrap());
            }
            let kind = match take(&mut bytes, 1)?[0] {
                0 => Portal::Nether,
                1 => Portal::End,
                2 => Portal::Gateway,
                _ => return Err(bad()),
            };
            tile.portals.push((at, kind));
        }
        if !bytes.is_empty() {
            return Err(bad());
        }
        Ok(tile)
    }
    struct Backing {
        store: Store,
        reads: BTreeMap<RecordKey, (u64, Arc<Tile>)>,
        read_bytes: usize,
        clock: u64,
        limit: usize,
        error: Option<String>,
    }
    impl Backing {
        fn get(&mut self, key: RecordKey) -> Option<Arc<Tile>> {
            self.clock += 1;
            if let Some((stamp, tile)) = self.reads.get_mut(&key) {
                *stamp = self.clock;
                return Some(tile.clone());
            }
            let tile = match self
                .store
                .read(key)
                .and_then(|data| data.map(|b| tile_decode(&b)).transpose())
            {
                Ok(Some(tile)) => Arc::new(tile),
                Ok(None) => return None,
                Err(e) => {
                    self.error = Some(e.to_string());
                    return None;
                }
            };
            let bytes = tile_bytes(&tile);
            while self.read_bytes + bytes > self.limit {
                let Some(key) = self
                    .reads
                    .iter()
                    .min_by_key(|(_, (stamp, _))| *stamp)
                    .map(|(key, _)| *key)
                else {
                    break;
                };
                self.read_bytes -= tile_bytes(&self.reads.remove(&key).unwrap().1);
            }
            self.read_bytes += bytes;
            self.reads.insert(key, (self.clock, tile.clone()));
            Some(tile)
        }
    }
    type TileCursor = Option<((i32, i32), Option<Arc<Tile>>)>;
    type Key = (Dimension, Layer, i32, i32);
    #[derive(Default)]
    pub struct Cache {
        tiles: BTreeMap<Key, (u64, Arc<Tile>)>,
        lod_tiles: BTreeMap<(Dimension, LodKey), (u64, Arc<Tile>)>,
        clock: u64,
        backing: Option<RefCell<Backing>>,
        limit: usize,
        paged: bool,
    }
    impl Cache {
        fn open(
            directory: &Path,
            world: &str,
            limit: usize,
        ) -> Result<Self, container::LeaseError> {
            Ok(Self {
                backing: Some(RefCell::new(Backing {
                    store: Store::open(directory, world)?,
                    reads: BTreeMap::new(),
                    read_bytes: 0,
                    clock: 0,
                    limit: limit / 4,
                    error: None,
                })),
                limit,
                ..Self::default()
            })
        }
        fn bytes(&self) -> usize {
            self.tiles
                .values()
                .chain(self.lod_tiles.values())
                .map(|(_, tile)| tile_bytes(tile))
                .sum()
        }
        fn flush(&mut self) -> io::Result<()> {
            if let Some(backing) = &self.backing {
                backing.borrow_mut().store.commit()?;
            }
            Ok(())
        }
        fn save(&mut self, key: RecordKey, tile: Arc<Tile>) -> io::Result<()> {
            if let Some(backing) = &self.backing {
                let mut backing = backing.borrow_mut();
                if let Some((_, old)) = backing.reads.remove(&key) {
                    backing.read_bytes -= tile_bytes(&old);
                }
                backing.store.put(key, tile_encode(&tile)?)?;
            }
            Ok(())
        }
        fn trim(&mut self) -> io::Result<()> {
            if self.limit == 0 {
                return Ok(());
            }
            let pending = self
                .backing
                .as_ref()
                .map_or(0, |b| b.borrow().store.pending_bytes());
            if pending > self.limit / 4 {
                self.flush()?;
            }
            while self.bytes() > self.limit / 2 {
                let fine = self
                    .tiles
                    .iter()
                    .min_by_key(|(_, (stamp, _))| *stamp)
                    .map(|(key, (stamp, _))| (*key, *stamp));
                let coarse = self
                    .lod_tiles
                    .iter()
                    .min_by_key(|(_, (stamp, _))| *stamp)
                    .map(|(key, (stamp, _))| (*key, *stamp));
                match (fine, coarse) {
                    (Some((key, a)), Some((_, b))) if a <= b => {
                        self.tiles.remove(&key);
                    }
                    (_, Some((key, _))) => {
                        self.lod_tiles.remove(&key);
                    }
                    (Some((key, _)), _) => {
                        self.tiles.remove(&key);
                    }
                    _ => break,
                }
            }
            Ok(())
        }
        fn clear_payloads(&mut self) {
            self.tiles.clear();
            self.lod_tiles.clear();
            if let Some(b) = &self.backing {
                let mut b = b.borrow_mut();
                b.reads.clear();
                b.read_bytes = 0;
            }
            self.clock += 1;
        }
        fn tile_at(&self, key: Key) -> Option<Arc<Tile>> {
            self.tiles.get(&key).map(|(_, t)| t.clone()).or_else(|| {
                self.backing
                    .as_ref()?
                    .borrow_mut()
                    .get(record_key(key.0, key.1, key.2, key.3))
            })
        }
        fn coarse_at(&self, d: Dimension, key: LodKey) -> Option<Arc<Tile>> {
            self.lod_tiles
                .get(&(d, key))
                .map(|(_, t)| t.clone())
                .or_else(|| self.backing.as_ref()?.borrow_mut().get(lod_record(d, key)))
        }
        fn prepare(&mut self, view: &View) -> io::Result<()> {
            let Some(backing) = &self.backing else {
                return Ok(());
            };
            let region = view.atlas_region();
            // Bound even the temporary list when a zoomed-out view covers an
            // enormous history; raster reads query the remaining keys on demand.
            let candidates = (self.limit / 4096).max(1);
            let keys: Vec<_> = backing
                .borrow()
                .store
                .keys()
                .filter(|key| {
                    record_dimension(*key) == Some(view.dimension)
                        && (record_layer(*key) == Some(view.layer)
                            && intersects(
                                region,
                                [i64::from(key[3]) * 16, i64::from(key[4]) * 16, 16, 16],
                            )
                            || key[1] == 2
                                && view.layer == Layer::Surface
                                && intersects(
                                    region,
                                    LodKey {
                                        x: key[3],
                                        z: key[4],
                                        level: key[5].clamp(0, 22) as u8,
                                    }
                                    .bounds(),
                                ))
                })
                .take(candidates + 1)
                .collect();
            self.paged = keys.len() > candidates;
            {
                let mut b = backing.borrow_mut();
                let border = [
                    region[0] - 16,
                    region[1] - 16,
                    region[2] + 32,
                    region[3] + 32,
                ];
                b.reads.retain(|key, _| {
                    record_dimension(*key) == Some(view.dimension)
                        && (record_layer(*key) == Some(view.layer)
                            && intersects(
                                border,
                                [i64::from(key[3]) * 16, i64::from(key[4]) * 16, 16, 16],
                            )
                            || key[1] == 2
                                && view.layer == Layer::Surface
                                && intersects(
                                    border,
                                    LodKey {
                                        x: key[3],
                                        z: key[4],
                                        level: key[5] as u8,
                                    }
                                    .bounds(),
                                ))
                });
                b.read_bytes = b.reads.values().map(|(_, tile)| tile_bytes(tile)).sum();
            }
            // Keep only the requested layer and its padded display region.
            self.tiles.retain(|(d, l, x, z), _| {
                *d == view.dimension
                    && *l == view.layer
                    && intersects(region, [i64::from(*x) * 16, i64::from(*z) * 16, 16, 16])
            });
            self.lod_tiles.retain(|(d, k), _| {
                *d == view.dimension
                    && view.layer == Layer::Surface
                    && intersects(region, k.bounds())
            });
            let mut used = self.bytes();
            for key in keys.into_iter().take(candidates) {
                let fine = record_layer(key).is_some_and(|l| l == view.layer)
                    && intersects(
                        region,
                        [i64::from(key[3]) * 16, i64::from(key[4]) * 16, 16, 16],
                    );
                let lod = LodKey {
                    x: key[3],
                    z: key[4],
                    level: key[5].clamp(0, 22) as u8,
                };
                let coarse =
                    key[1] == 2 && view.layer == Layer::Surface && intersects(region, lod.bounds());
                if !fine && !coarse {
                    continue;
                }
                if fine
                    && self
                        .tiles
                        .contains_key(&(view.dimension, view.layer, key[3], key[4]))
                    || coarse && self.lod_tiles.contains_key(&(view.dimension, lod))
                {
                    continue;
                }
                let Some(tile) = self.backing.as_ref().unwrap().borrow_mut().get(key) else {
                    continue;
                };
                let size = tile_bytes(&tile);
                if used.saturating_add(size) > self.limit / 2 {
                    self.paged = true;
                    continue;
                }
                used += size;
                self.clock += 1;
                if fine {
                    self.tiles.insert(
                        (view.dimension, view.layer, key[3], key[4]),
                        (self.clock, tile),
                    );
                } else {
                    self.lod_tiles
                        .insert((view.dimension, lod), (self.clock, tile));
                }
            }
            Ok(())
        }

        pub fn insert(
            &mut self,
            dimension: Dimension,
            layer: Layer,
            chunk: [i32; 2],
            mut tile: Tile,
        ) {
            let key = (dimension, layer, chunk[0], chunk[1]);
            if let Some(previous) = self.tile_at(key)
                && !previous.biomes.is_empty()
            {
                tile.biomes.resize(256, None);
                for (i, biome) in tile.biomes.iter_mut().enumerate() {
                    if biome.is_none() {
                        *biome = previous.biomes.get(i).cloned().flatten();
                    }
                }
            }
            self.clock += 1;
            self.tiles.insert(key, (self.clock, Arc::new(tile)));
        }
        pub fn insert_lod(&mut self, dimension: Dimension, key: LodKey, tile: Tile) -> bool {
            if key.level > 22
                || tile.pixels.len() != 1024
                || !tile.pixels.chunks_exact(4).any(|p| p[3] != 0)
            {
                return false;
            }
            if !self.lod_tiles.contains_key(&(dimension, key))
                && let Some(old) = self.coarse_at(dimension, key)
            {
                self.lod_tiles.insert((dimension, key), (self.clock, old));
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
            let mut fine: BTreeMap<Key, Arc<Tile>> = self
                .tiles
                .iter()
                .map(|(key, (_, tile))| (*key, tile.clone()))
                .collect();
            let mut coarse: BTreeMap<(Dimension, LodKey), Arc<Tile>> = self
                .lod_tiles
                .iter()
                .map(|(key, (_, tile))| (*key, tile.clone()))
                .collect();
            if let Some(backing) = &self.backing {
                for (key, (_, tile)) in &backing.borrow().reads {
                    let Some(d) = record_dimension(*key) else {
                        continue;
                    };
                    if let Some(layer) = record_layer(*key) {
                        fine.entry((d, layer, key[3], key[4]))
                            .or_insert_with(|| tile.clone());
                    } else if key[1] == 2 {
                        coarse
                            .entry((
                                d,
                                LodKey {
                                    x: key[3],
                                    z: key[4],
                                    level: key[5] as u8,
                                },
                            ))
                            .or_insert_with(|| tile.clone());
                    }
                }
            }
            Coverage {
                session: 0,
                detailed: fine.keys().copied().collect(),
                detailed_data: fine,
                lod: coarse
                    .iter()
                    .map(|((d, key), tile)| {
                        let mut mask = [0; 4];
                        for (i, pixel) in tile.pixels.chunks_exact(4).enumerate() {
                            if pixel[3] != 0 {
                                mask[i / 64] |= 1 << (i % 64);
                            }
                        }
                        (*d, *key, mask)
                    })
                    .collect(),
                lod_data: coarse,
            }
        }
        fn lod_pixel(
            &self,
            dimension: Dimension,
            layer: Layer,
            block: [i32; 2],
        ) -> Option<[u8; 4]> {
            if layer != Layer::Surface {
                return None;
            }
            for level in 0..=22 {
                let size = 16_i64 << level;
                let key = LodKey {
                    x: (i64::from(block[0]).div_euclid(size)) as i32,
                    z: (i64::from(block[1]).div_euclid(size)) as i32,
                    level,
                };
                if let Some(tile) = self.coarse_at(dimension, key) {
                    let cell = key.cell(block)?;
                    let pixel = tile.pixels.get(cell * 4..cell * 4 + 4)?;
                    if pixel[3] != 0 {
                        return Some(pixel.try_into().unwrap());
                    }
                }
            }
            None
        }
        fn gpu_atlas_fits(&self, view: &View) -> bool {
            if self.paged {
                return false;
            }
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
                pixels,
                height,
                chunks,
                coarse,
                region,
                width: 1024,
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
            let mut tile: TileCursor = None;
            let mut north: TileCursor = None;
            let mut west: TileCursor = None;
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
                    if tile.as_ref().is_none_or(|(key, _)| *key != chunk) {
                        tile = Some((
                            chunk,
                            self.tile_at((view.dimension, view.layer, chunk.0, chunk.1)),
                        ));
                    }
                    let current = tile.as_ref().and_then(|(_, tile)| tile.as_ref());
                    let i = (y * width + x) * 4;
                    let local = (bz.rem_euclid(16) * 16 + bx.rem_euclid(16)) as usize;
                    let Some(current) = current else {
                        if let Some(color) = self.lod_pixel(view.dimension, view.layer, [bx, bz]) {
                            pixels[i..i + 4].copy_from_slice(&color);
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
                        Some(current.clone())
                    } else {
                        if north.as_ref().is_none_or(|(key, _)| *key != north_chunk) {
                            north = Some((
                                north_chunk,
                                self.tile_at((
                                    view.dimension,
                                    view.layer,
                                    north_chunk.0,
                                    north_chunk.1,
                                )),
                            ));
                        }
                        north.as_ref().and_then(|(_, tile)| tile.clone())
                    };
                    let west_x = bx - 1;
                    let west_chunk = (west_x.div_euclid(16), bz.div_euclid(16));
                    let west_tile = if west_chunk == chunk {
                        Some(current.clone())
                    } else {
                        if west.as_ref().is_none_or(|(key, _)| *key != west_chunk) {
                            west = Some((
                                west_chunk,
                                self.tile_at((
                                    view.dimension,
                                    view.layer,
                                    west_chunk.0,
                                    west_chunk.1,
                                )),
                            ));
                        }
                        west.as_ref().and_then(|(_, tile)| tile.clone())
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
                    let relief = f32::from(north_height.saturating_sub(height).clamp(-64, 64) as i8)
                        * -0.085
                        + f32::from(west_height.saturating_sub(height).clamp(-64, 64) as i8)
                            * -0.05;
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
                raster_source: None,
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

    #[derive(Clone, PartialEq, Eq)]
    struct Binding {
        session: u64,
        world: String,
        dimension: Dimension,
        limit: usize,
    }
    pub struct Service {
        binding: Arc<Mutex<Option<Binding>>>,
        status: Arc<Mutex<Option<String>>>,
        paused: Arc<std::sync::atomic::AtomicBool>,
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
            Self::with_directory(None)
        }
        pub fn with_directory(directory: Option<std::path::PathBuf>) -> Self {
            let (tx, rx) = mpsc::sync_channel(8);
            let latest = Arc::new(Mutex::new(None::<View>));
            let output = Arc::new(Mutex::new(None));
            let session = Arc::new(AtomicU64::new(0));
            let resident = Arc::new(Mutex::new(BTreeSet::new()));
            let portals = Arc::new(Mutex::new(BTreeMap::new()));
            let coverage = Arc::new(Mutex::new(Arc::new(Coverage::default())));
            let binding = Arc::new(Mutex::new(None::<Binding>));
            let status = Arc::new(Mutex::new(None::<String>));
            let (input, result, active) = (latest.clone(), output.clone(), session.clone());
            let (residency, coverage_state, portal_points) =
                (resident.clone(), coverage.clone(), portals.clone());
            let (desired, errors) = (binding.clone(), status.clone());
            let paused = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let paused_state = paused.clone();
            let worker = thread::Builder::new().name("map-terrain".into()).spawn(move || {
                    let mut cache = Cache::default();
                    let mut current = 0;
                    let mut configured = None::<Binding>;
                    let mut serial = 0;
                    let mut atlas = None::<AtlasCache>;
                    let mut last_commit = Instant::now();
                    let mut paused = false;
                    loop {
                        let first = match rx.recv_timeout(Duration::from_millis(100)) {
                            Ok(job) => Some(job),
                            Err(mpsc::RecvTimeoutError::Timeout) => None,
                            Err(mpsc::RecvTimeoutError::Disconnected) => {
                                if let Err(e) = cache.flush() {
                                    eprintln!("Map cache flush on exit failed: {e}");
                                    *errors.lock().unwrap_or_else(|e| e.into_inner()) = Some(e.to_string());
                                }
                                break;
                            }
                        };
                        let wanted = desired.lock().unwrap_or_else(|e| e.into_inner()).clone();
                        let next = active.load(Ordering::Acquire);
                        let mut changed = false;
                        if current != next
                            || configured.as_ref().map(|b| (&b.world, b.session))
                                != wanted.as_ref().map(|b| (&b.world, b.session))
                        {
                            if let Err(e) = cache.flush() {
                                paused = true;
                                *errors.lock().unwrap_or_else(|e| e.into_inner()) =
                                    Some(format!("Cache write failed; sampling paused: {e}"));
                            }
                            if !paused {
                                cache = Cache::default();
                                *errors.lock().unwrap_or_else(|e| e.into_inner()) = None;
                                if let (Some(directory), Some(b)) = (&directory, &wanted) {
                                    if b.world.is_empty() {
                                        cache.limit=b.limit;
                                        *errors.lock().unwrap_or_else(|e|e.into_inner())=Some("Unknown world identity; temporary RAM map only".into());
                                    } else {match Cache::open(directory, &b.world, b.limit) {
                                        Ok(opened) => cache = opened,
                                        Err(container::LeaseError::Busy) => {
                                            cache.limit = b.limit;
                                            *errors.lock().unwrap_or_else(|e| e.into_inner()) = Some(
                                                "Cache is used by another client; temporary RAM map only".into(),
                                            );
                                        }
                                        Err(container::LeaseError::Io(e)) => {
                                            paused = true;
                                            *errors.lock().unwrap_or_else(|e| e.into_inner()) =
                                                Some(format!("Cannot open cache; sampling paused: {e}"));
                                        }
                                    }
                                    }
                                }
                                current = next;
                                configured = wanted.clone();
                                atlas = None;
                                changed = true;
                                last_commit = Instant::now();
                            }
                        }
                        if let (Some(old), Some(new)) = (&configured.clone(), &wanted) {
                            if old.dimension != new.dimension && !paused {
                                if let Err(e) = cache.flush() {
                                    paused = true;
                                    *errors.lock().unwrap_or_else(|e| e.into_inner()) =
                                        Some(format!("Cache write failed; sampling paused: {e}"));
                                } else {
                                    cache.clear_payloads();
                                    atlas = None;
                                    changed = true;
                                    *coverage_state.lock().unwrap_or_else(|e| e.into_inner()) =
                                        Arc::new(Coverage::default());
                                    residency.lock().unwrap_or_else(|e| e.into_inner()).clear();
                                    portal_points
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner())
                                        .clear();
                                    *result.lock().unwrap_or_else(|e| e.into_inner()) = None;
                                    configured = wanted.clone();
                                    last_commit = Instant::now();
                                }
                            }
                            if old.limit != new.limit {
                                cache.limit = new.limit;
                                if let Some(backing) = &cache.backing {
                                    let mut b = backing.borrow_mut();
                                    b.limit = new.limit / 4;
                                    b.reads.clear();
                                    b.read_bytes = 0;
                                }
                                if let Err(e) = cache.trim() {
                                    paused = true;
                                    *errors.lock().unwrap_or_else(|e| e.into_inner()) = Some(e.to_string());
                                }
                                configured = wanted.clone();
                                changed = true;
                            }
                        }
                        let mut jobs: Vec<_> = first.into_iter().collect();
                        jobs.extend(rx.try_iter().take(7));
                        for job in jobs {
                            if paused {
                                continue;
                            }
                            let saved = match job {
                                Job::Chunk {
                                    session,
                                    dimension,
                                    layer,
                                    chunk,
                                    sample,
                                } if session == current
                                    && session == active.load(Ordering::Acquire)
                                    && configured.as_ref().is_none_or(|b| b.dimension == dimension) =>
                                {
                                    let tile = sample();
                                    if session != active.load(Ordering::Acquire) {
                                        continue;
                                    }
                                    cache.insert(dimension, layer, chunk, tile);
                                    changed = true;
                                    let tile = cache.tiles[&(dimension, layer, chunk[0], chunk[1])]
                                        .1
                                        .clone();
                                    cache.save(record_key(dimension, layer, chunk[0], chunk[1]), tile)
                                }
                                Job::Lod {
                                    session,
                                    dimension,
                                    key,
                                    sample,
                                } if session == current
                                    && session == active.load(Ordering::Acquire)
                                    && configured.as_ref().is_none_or(|b| b.dimension == dimension) =>
                                {
                                    let tile = sample();
                                    if session != active.load(Ordering::Acquire) {
                                        continue;
                                    }
                                    if cache.insert_lod(dimension, key, tile) {
                                        changed = true;
                                        let tile = cache.lod_tiles[&(dimension, key)].1.clone();
                                        cache.save(lod_record(dimension, key), tile)
                                    } else {
                                        Ok(())
                                    }
                                }
                                _ => Ok(()),
                            };
                            if let Err(e) = saved.and_then(|_| cache.trim()) {
                                paused = true;
                                *errors.lock().unwrap_or_else(|e| e.into_inner()) =
                                    Some(format!("Cache write failed; sampling paused: {e}"));
                            }
                        }
                        if !paused && last_commit.elapsed() >= Duration::from_secs(5) {
                            if let Err(e) = cache.flush() {
                                paused = true;
                                *errors.lock().unwrap_or_else(|e| e.into_inner()) =
                                    Some(format!("Cache write failed; sampling paused: {e}"));
                            }
                            last_commit = Instant::now();
                        }
                        paused_state.store(paused, Ordering::Release);
                        let Some(view) = input.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
                            continue;
                        };
                        if view.session != current || view.session != active.load(Ordering::Acquire) {
                            continue;
                        }
                        let same = result
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .as_ref()
                            .is_some_and(|f: &Arc<Frame>| f.view == view);
                        if same && !changed {
                            continue;
                        }
                        if !paused && let Err(e) = cache.prepare(&view) {
                            *errors.lock().unwrap_or_else(|e| e.into_inner()) = Some(e.to_string());
                        }
                        serial += 1;
                        let frame = if view.large && cache.gpu_atlas_fits(&view) {
                            let key = (view.session, view.dimension, view.layer, cache.clock);
                            if atlas
                                .as_ref()
                                .is_none_or(|a| a.key != key || !a.terrain.covers(&view))
                            {
                                atlas = Some(AtlasCache {
                                    key,
                                    terrain: Arc::new(cache.terrain_atlas(&view, serial)),
                                });
                            }
                            let terrain = atlas.as_ref().unwrap().terrain.clone();
                            Arc::new(Frame {
                                raster_source: None,
                                serial,
                                view,
                                pixels: Vec::new(),
                                known: terrain.chunks.len() + terrain.coarse.len(),
                                atlas: Some(terrain),
                            })
                        } else {
                            atlas = None;
                            Arc::new(cache.render(view, serial))
                        };
                        if let Some(backing) = &cache.backing
                            && let Some(error) = backing.borrow_mut().error.take()
                        {
                            *errors.lock().unwrap_or_else(|e| e.into_inner()) =
                                Some(format!("Damaged cache record skipped: {error}"));
                        }
                        if current == active.load(Ordering::Acquire) {
                            let mut summary = cache.coverage();
                            summary.session = current;
                            let mut points = BTreeMap::new();
                            for ((d, _, _, _), tile) in &summary.detailed_data {
                                for (at, kind) in &tile.portals {
                                    points.insert((*d, at[0], at[2], *kind), *at);
                                }
                            }
                            *portal_points.lock().unwrap_or_else(|e| e.into_inner()) = points;
                            *residency.lock().unwrap_or_else(|e| e.into_inner()) = summary.detailed.clone();
                            *coverage_state.lock().unwrap_or_else(|e| e.into_inner()) = Arc::new(summary);
                            *result.lock().unwrap_or_else(|e| e.into_inner()) = Some(frame);
                        }
                    }
            }).map_err(|e| eprintln!("Map worker failed: {e}")).ok();
            if worker.is_none() {
                paused.store(true, Ordering::Release);
                *status.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some("Map worker unavailable; sampling paused".into());
            }
            Self {
                tx: Mutex::new(Some(tx)),
                worker: Mutex::new(worker),
                latest,
                output,
                session,
                resident,
                coverage,
                portals,
                binding,
                status,
                paused,
            }
        }
        pub fn configure(&self, session: u64, world: &str, dimension: Dimension, limit_mib: u16) {
            let value = Binding {
                session,
                world: world.into(),
                dimension,
                limit: usize::from(limit_mib.clamp(16, 512)) * 1024 * 1024,
            };
            let mut binding = self.binding.lock().unwrap_or_else(|e| e.into_inner());
            if binding.as_ref() == Some(&value) {
                return;
            }
            *binding = Some(value);
            drop(binding);
            if let Some(tx) = self.tx.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                let _ = tx.try_send(Job::Wake);
            }
        }
        pub fn status(&self) -> Option<String> {
            self.status
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        }
        pub fn reset(&self, session: u64) {
            *self.binding.lock().unwrap_or_else(|e| e.into_inner()) = None;
            *self.latest.lock().unwrap_or_else(|e| e.into_inner()) = None;
            if let Some(tx) = self.tx.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                let _ = tx.try_send(Job::Wake);
            }
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
            if self.paused.load(Ordering::Acquire) {
                return false;
            }
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
            if self.paused.load(Ordering::Acquire) {
                return false;
            }
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
        struct CacheDir(std::path::PathBuf);
        impl CacheDir {
            fn new(label: &str) -> Self {
                static NEXT: AtomicU64 = AtomicU64::new(0);
                let dir = std::env::temp_dir().join(format!(
                    "mwwr-cache-{label}-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                std::fs::create_dir(&dir).unwrap();
                Self(dir)
            }
        }
        impl Drop for CacheDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        fn saved_tile() -> Tile {
            Tile {
                pixels: [80, 120, 40, 255].repeat(256),
                heights: [42; 256],
                biomes: vec![Some(Arc::from("minecraft:forest")); 256],
                portals: vec![([-16, 42, 0], Portal::Nether)],
                ..Tile::default()
            }
        }
        fn save_chunk(cache: &mut Cache, d: Dimension, l: Layer, at: [i32; 2], tile: Tile) {
            cache.insert(d, l, at, tile);
            let tile = cache.tiles[&(d, l, at[0], at[1])].1.clone();
            cache.save(record_key(d, l, at[0], at[1]), tile).unwrap();
            cache.trim().unwrap();
        }
        #[test]
        fn persisted_layers_biomes_portals_and_lod_reload_after_eviction() {
            let dir = CacheDir::new("reopen");
            let world = "server:reopen:25565";
            let limit = 1024 * 1024;
            let mut cache = Cache::open(&dir.0, world, limit).unwrap();
            save_chunk(
                &mut cache,
                Dimension::Overworld,
                Layer::Surface,
                [-1, 0],
                saved_tile(),
            );
            let mut underground = saved_tile();
            underground.heights = [12; 256];
            save_chunk(
                &mut cache,
                Dimension::Overworld,
                Layer::Below(16),
                [-1, 0],
                underground,
            );
            let lod = LodKey {
                x: 0,
                z: 0,
                level: 3,
            };
            let tile = saved_tile();
            cache.insert_lod(Dimension::Nether, lod, tile);
            cache
                .save(
                    lod_record(Dimension::Nether, lod),
                    cache.lod_tiles[&(Dimension::Nether, lod)].1.clone(),
                )
                .unwrap();
            cache.flush().unwrap();
            cache.clear_payloads();
            assert_eq!(cache.bytes(), 0);
            drop(cache);
            let mut cache = Cache::open(&dir.0, world, limit).unwrap();
            let mut v = view(Dimension::Overworld);
            cache.prepare(&v).unwrap();
            assert_eq!(cache.render(v.clone(), 1).known, 256);
            let coverage = cache.coverage();
            assert_eq!(
                coverage
                    .detailed_biome(Dimension::Overworld, Layer::Surface, [-16, 0])
                    .as_deref(),
                Some("minecraft:forest")
            );
            assert_eq!(
                cache
                    .tile_at((Dimension::Overworld, Layer::Surface, -1, 0))
                    .unwrap()
                    .portals
                    .len(),
                1
            );
            v.layer = Layer::Below(16);
            cache.prepare(&v).unwrap();
            assert_eq!(
                cache
                    .tile_at((Dimension::Overworld, v.layer, -1, 0))
                    .unwrap()
                    .heights,
                [12; 256]
            );
            cache.clear_payloads();
            let tile = cache.coarse_at(Dimension::Nether, lod).unwrap();
            assert_eq!(tile.heights, [42; 256]);
            // Refresh with unavailable biome information retains persisted names.
            let mut fresh = saved_tile();
            fresh.biomes.clear();
            save_chunk(
                &mut cache,
                Dimension::Overworld,
                Layer::Surface,
                [-1, 0],
                fresh,
            );
            assert_eq!(
                cache.tiles[&(Dimension::Overworld, Layer::Surface, -1, 0)]
                    .1
                    .biomes[0]
                    .as_deref(),
                Some("minecraft:forest")
            );
        }
        #[test]
        fn ordinary_disk_tiles_use_the_gpu_atlas_within_the_ram_budget() {
            let dir = CacheDir::new("atlas-budget");
            let mut cache = Cache::open(&dir.0, "save:atlas-budget", 16 * 1024 * 1024).unwrap();
            for x in 0..300 {
                save_chunk(
                    &mut cache,
                    Dimension::Overworld,
                    Layer::Surface,
                    [x, 0],
                    saved_tile(),
                );
            }
            cache.flush().unwrap();
            cache.clear_payloads();
            let mut v = view(Dimension::Overworld);
            v.center = [2400.0, 8.0];
            v.span = [4800.0, 16.0];
            cache.prepare(&v).unwrap();
            assert_eq!(cache.tiles.len(), 300);
            assert!(!cache.paged);
            assert!(cache.gpu_atlas_fits(&v));
        }
        #[test]
        fn raster_camera_moves_without_reuploading_the_worker_image() {
            let source = Arc::new(Cache::default().render(view(Dimension::Overworld), 1));
            let mut shown = None;
            assert!(present_frame(&mut shown, source.clone(), |_, _, _| true));
            let mut moved = source.view.clone();
            moved.center[0] += 8.0;
            let (src, dst) = raster_quad(&source.view, &moved, [0.0, 0.0, 160.0, 160.0]).unwrap();
            assert_eq!(src, [8.0, 0.0, 8.0, 16.0]);
            assert_eq!(dst, [0.0, 0.0, 80.0, 160.0]);
            let next = Arc::new(Frame {
                raster_source: Some(source.clone()),
                serial: source.serial,
                view: moved,
                pixels: Vec::new(),
                known: source.known,
                atlas: None,
            });
            assert!(present_frame(&mut shown, next, |_, _, _| panic!(
                "camera-only upload"
            )));
            let mut wide = source.view.clone();
            wide.span = [32.0; 2];
            assert_eq!(
                raster_quad(&source.view, &wide, [0.0, 0.0, 160.0, 160.0])
                    .unwrap()
                    .1,
                [40.0, 40.0, 80.0, 80.0]
            );
            wide.center = [1000.0; 2];
            assert!(raster_quad(&source.view, &wide, [0.0, 0.0, 160.0, 160.0]).is_none());
        }
        #[test]
        fn huge_view_pages_tiles_under_budget_and_detailed_void_still_masks_lod() {
            let dir = CacheDir::new("budget");
            let limit = 1024 * 1024;
            let mut cache = Cache::open(&dir.0, "save:budget", limit).unwrap();
            for x in 0..128 {
                save_chunk(
                    &mut cache,
                    Dimension::Overworld,
                    Layer::Surface,
                    [x, 0],
                    saved_tile(),
                );
            }
            let mut empty = saved_tile();
            empty.pixels.fill(0);
            save_chunk(
                &mut cache,
                Dimension::Overworld,
                Layer::Surface,
                [0, 0],
                empty,
            );
            let key = LodKey {
                x: 0,
                z: 0,
                level: 7,
            };
            cache.insert_lod(Dimension::Overworld, key, saved_tile());
            cache
                .save(
                    lod_record(Dimension::Overworld, key),
                    cache.lod_tiles[&(Dimension::Overworld, key)].1.clone(),
                )
                .unwrap();
            cache.flush().unwrap();
            cache.clear_payloads();
            let mut v = view(Dimension::Overworld);
            v.center = [1024.0, 8.0];
            v.span = [2048.0, 16.0];
            v.width = 1024;
            cache.prepare(&v).unwrap();
            assert!(cache.paged);
            assert!(!cache.gpu_atlas_fits(&v));
            let frame = cache.render(v, 1);
            assert_eq!(frame.known, 1024 * 16);
            assert!(frame.pixels[..8 * 4].iter().all(|b| *b == 0));
            let backing = cache.backing.as_ref().unwrap().borrow();
            let reads = backing
                .reads
                .values()
                .map(|(_, t)| tile_bytes(t))
                .sum::<usize>();
            assert!(cache.bytes() + reads + backing.store.pending_bytes() <= limit);
        }
        fn wait_frame(service: &Service, predicate: impl Fn(&Frame) -> bool) -> Arc<Frame> {
            let start = Instant::now();
            loop {
                if let Some(frame) = service.frame()
                    && predicate(&frame)
                {
                    return frame;
                }
                assert!(
                    start.elapsed() < Duration::from_secs(5),
                    "map worker timeout: {:?}",
                    service.status()
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        #[test]
        fn service_dimension_switch_flushes_and_releases_old_payload_then_restart_loads_disk() {
            let dir = CacheDir::new("service");
            let service = Service::with_directory(Some(dir.0.clone()));
            service.reset(1);
            service.configure(1, "server:restart", Dimension::Overworld, 64);
            let mut v = view(Dimension::Overworld);
            v.large = false;
            service.request(v.clone());
            assert!(service.submit(1, Dimension::Overworld, Layer::Surface, [-1, 0], saved_tile));
            let old = wait_frame(&service, |f| f.known == 256);
            drop(old);
            let old_coverage = service.coverage();
            let weak = Arc::downgrade(old_coverage.detailed_data.values().next().unwrap());
            drop(old_coverage);
            service.configure(1, "server:restart", Dimension::Nether, 64);
            v.dimension = Dimension::Nether;
            service.request(v.clone());
            drop(wait_frame(&service, |f| {
                f.view.dimension == Dimension::Nether
            }));
            assert!(weak.upgrade().is_none(), "old dimension tiles retained");
            assert!(service.coverage().detailed_data.is_empty());
            service.shutdown();
            drop(service);
            let reopened = Service::with_directory(Some(dir.0.clone()));
            reopened.reset(2);
            reopened.configure(2, "server:restart", Dimension::Overworld, 64);
            v.session = 2;
            v.dimension = Dimension::Overworld;
            reopened.request(v);
            assert_eq!(wait_frame(&reopened, |f| f.known == 256).known, 256);
            reopened.shutdown();
            assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
        }
        #[test]
        fn failed_writer_keeps_pending_data_and_service_stops_accepting_samples() {
            let dir = CacheDir::new("write-error");
            let anonymous_dir = dir.0.join("anonymous");
            let anonymous = Service::with_directory(Some(anonymous_dir.clone()));
            anonymous.reset(1);
            anonymous.configure(1, "", Dimension::Overworld, 16);
            let mut anonymous_view = view(Dimension::Overworld);
            anonymous_view.large = false;
            anonymous.request(anonymous_view);
            assert!(anonymous.submit(1, Dimension::Overworld, Layer::Surface, [-1, 0], saved_tile));
            drop(wait_frame(&anonymous, |f| f.known == 256));
            assert!(anonymous.status().is_some());
            anonymous.shutdown();
            assert!(
                !anonymous_dir.exists(),
                "unknown identity must not create an unscoped cache"
            );

            let mut cache = Cache::open(&dir.0, "server:write-error", 1024 * 1024).unwrap();
            save_chunk(
                &mut cache,
                Dimension::Overworld,
                Layer::Surface,
                [0, 0],
                saved_tile(),
            );
            let bytes = cache.bytes();
            let pending = cache
                .backing
                .as_ref()
                .unwrap()
                .borrow()
                .store
                .pending_bytes();
            cache
                .backing
                .as_ref()
                .unwrap()
                .borrow_mut()
                .store
                .write_budget = Some(0);
            assert!(cache.flush().is_err());
            assert_eq!(cache.bytes(), bytes);
            assert_eq!(
                cache
                    .backing
                    .as_ref()
                    .unwrap()
                    .borrow()
                    .store
                    .pending_bytes(),
                pending
            );
            drop(cache);
            let service = Service::with_directory(Some(dir.0.clone()));
            service.reset(1);
            service.configure(1, "server:encode-error", Dimension::Overworld, 64);
            assert!(service.submit(
                1,
                Dimension::Overworld,
                Layer::Surface,
                [0, 0],
                Tile::default
            ));
            let start = Instant::now();
            while !service.paused.load(Ordering::Acquire) {
                assert!(start.elapsed() < Duration::from_secs(5));
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(service.status().is_some());
            assert!(!service.submit(1, Dimension::Overworld, Layer::Surface, [1, 0], saved_tile));
            assert!(!service.submit_lod(
                1,
                Dimension::Overworld,
                LodKey {
                    x: 0,
                    z: 0,
                    level: 1
                },
                saved_tile
            ));
            service.shutdown();
        }
        #[test]
        fn tile_decoder_rejects_truncation_and_bounded_strings() {
            let bytes = tile_encode(&saved_tile()).unwrap();
            for len in [0, 4, 100, 1028, 2050, bytes.len() - 1] {
                assert!(tile_decode(&bytes[..len]).is_err());
            }
            let mut bad = saved_tile();
            bad.biomes[0] = Some(Arc::from("a".repeat(257)));
            assert!(tile_encode(&bad).is_err());
            assert_eq!(tile_decode(&bytes).unwrap().heights, [42; 256]);
            // The sampler can retain each of three portal kinds per column.
            let mut mixed = saved_tile();
            mixed.portals.clear();
            for x in 0..16 {
                for z in 0..16 {
                    for kind in [Portal::Nether, Portal::End, Portal::Gateway] {
                        mixed.portals.push(([x, 42, z], kind));
                    }
                }
            }
            assert_eq!(
                tile_decode(&tile_encode(&mixed).unwrap())
                    .unwrap()
                    .portals
                    .len(),
                MAX_TILE_PORTALS
            );
            assert!(tile_bytes(&mixed) <= 128 * 1024);
        }

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
            }
        }
        #[test]
        fn grass_and_leaves_take_the_biome_colour_with_distinct_birch_and_oak() {
            assert_eq!(
                block_map_color("minecraft:grass_block", 0x7fb238, Some(0x91bd59), None),
                0x91bd59
            );
            assert_eq!(
                block_map_color("minecraft:grass_block", 0x7fb238, None, None),
                0x7fb238
            );
            assert_eq!(
                block_map_color("minecraft:stone", 0x707070, Some(0x91bd59), Some(0x59ae30)),
                0x707070
            );
            assert_eq!(
                block_map_color("minecraft:grass", 0x7fb238, Some(0xffffff), None),
                0xffffff
            );
            assert_eq!(
                block_map_color("minecraft:oak_leaves", 0x7fb238, None, Some(0x59ae30)),
                0x59ae30
            );
            assert_eq!(
                block_map_color("minecraft:birch_leaves", 0x7fb238, None, Some(0x59ae30)),
                0x80a755
            );
            assert_eq!(
                block_map_color("minecraft:spruce_leaves", 0x7fb238, None, None),
                0x619961
            );
            assert_eq!(
                block_map_color("minecraft:lily_pad", 0x3f7f3f, Some(0x91bd59), None),
                0x91bd59
            );
            assert_eq!(
                block_map_color("minecraft:vine", 0x3f6f2f, None, Some(0x4c763c)),
                0x4c763c
            );
            assert!(needs_grass_tint("minecraft:lily_pad"));
            assert!(needs_foliage_tint("minecraft:vine"));
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
        fn presentation_uploads_the_exact_layout_before_publishing_geometry() {
            let mut cache = terrain_fixture();
            cache.insert_lod(
                Dimension::Overworld,
                LodKey {
                    x: 2,
                    z: 0,
                    level: 2,
                },
                coarse_tile([25, 75, 125]),
            );
            let mut v = view(Dimension::Overworld);
            v.center = [64.0, 0.0];
            v.span = [512.0; 2];
            let make_frame = |atlas: Arc<TerrainAtlas>, view: View| {
                Arc::new(Frame {
                    raster_source: None,
                    serial: 7,
                    view,
                    pixels: Vec::new(),
                    known: atlas.chunks.len() + atlas.coarse.len(),
                    atlas: Some(atlas),
                })
            };
            let first_atlas = Arc::new(cache.terrain_atlas(&v, 7));
            let first = make_frame(first_atlas.clone(), v.clone());
            // Same dimensions and serial, different contents and slot offsets:
            // the old (serial, width) upload key incorrectly skipped this image.
            cache.insert(
                Dimension::Overworld,
                Layer::Surface,
                [-3, 0],
                coarse_tile([190, 30, 15]),
            );
            let second_atlas = Arc::new(cache.terrain_atlas(&v, 7));
            assert_eq!(
                (first_atlas.width, first_atlas.height),
                (second_atlas.width, second_atlas.height)
            );
            assert_ne!(first_atlas.pixels, second_atlas.pixels);
            let second = make_frame(second_atlas.clone(), v.clone());
            let mut shown = None;
            let mut gpu = Vec::new();
            assert!(present_frame(&mut shown, first.clone(), |_, _, pixels| {
                gpu = pixels.to_vec();
                true
            }));
            assert_eq!(gpu, first_atlas.pixels);
            assert!(!present_frame(&mut shown, second.clone(), |_, _, _| false));
            assert!(Arc::ptr_eq(shown.as_ref().unwrap(), &first));
            assert_eq!(gpu, first_atlas.pixels);
            assert!(present_frame(&mut shown, second.clone(), |_, _, pixels| {
                gpu = pixels.to_vec();
                true
            }));
            assert_eq!(gpu, shown.as_ref().unwrap().atlas.as_ref().unwrap().pixels);
            // Camera-only changes reuse the immutable texture at every scale.
            for span in [32.0, 64.0, 128.0, 256.0, 512.0, 128.0, 32.0] {
                let mut zoomed = v.clone();
                zoomed.span = [span; 2];
                assert!(present_frame(
                    &mut shown,
                    make_frame(second_atlas.clone(), zoomed),
                    |_, _, _| { panic!("camera movement must not upload another texture") }
                ));
                assert_eq!(gpu, shown.as_ref().unwrap().atlas.as_ref().unwrap().pixels);
            }
            // Raster fallback and return to atlas must both replace the image.
            let raster = Arc::new(cache.render(v, 7));
            assert!(present_frame(&mut shown, raster.clone(), |_, _, pixels| {
                gpu = pixels.to_vec();
                true
            }));
            assert_eq!(gpu, raster.pixels);
            assert!(present_frame(&mut shown, second.clone(), |_, _, pixels| {
                gpu = pixels.to_vec();
                true
            }));
            assert_eq!(gpu, second_atlas.pixels);
            // Resource reload invalidates both displayed geometry and texture.
            shown = None;
            gpu.clear();
            assert!(present_frame(&mut shown, second, |_, _, pixels| {
                gpu = pixels.to_vec();
                true
            }));
            assert_eq!(gpu, second_atlas.pixels);
        }
        #[test]
        fn detailed_texels_stay_identical_across_zoom_and_atlas_repacking() {
            let mut cache = terrain_fixture();
            for x in -64..64 {
                cache.insert(
                    Dimension::Overworld,
                    Layer::Surface,
                    [x, 3],
                    coarse_tile([x as u8, 30, 120]),
                );
            }
            let mut v = view(Dimension::Overworld);
            v.center = [8.0, 8.0];
            for span in [16.0, 32.0, 128.0, 512.0, 2048.0, 128.0, 16.0] {
                v.span = [span; 2];
                let atlas = cache.terrain_atlas(&v, 7);
                for (slot, chunk) in atlas.chunks.iter().enumerate() {
                    let tile = cache.render(
                        View {
                            center: chunk.map(|c| f64::from(c) * 16.0 + 8.0),
                            span: [16.0; 2],
                            width: 16,
                            height: 16,
                            ..v.clone()
                        },
                        0,
                    );
                    for row in 0..16 {
                        let from = ((slot / 64 * 16 + row) * atlas.width + slot % 64 * 16) * 4;
                        assert_eq!(
                            &atlas.pixels[from..from + 64],
                            &tile.pixels[row * 64..row * 64 + 64]
                        );
                    }
                    let tile_view = View {
                        center: tile.view.center,
                        span: [16.0; 2],
                        ..v.clone()
                    };
                    let (uv, _) = atlas
                        .quad(&tile_view, [0.0, 0.0, 16.0, 16.0], *chunk, slot)
                        .unwrap();
                    assert_eq!(uv[2..], [16.0, 16.0]);
                }
            }
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
                Some([20, 20, 20, 255])
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
                Some([10, 20, 30, 255])
            );
            assert_eq!(
                cache.lod_pixel(Dimension::Overworld, Layer::Surface, [4, 0]),
                Some([40, 50, 60, 255])
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
            for span in [256.0, 512.0, 1024.0, 4096.0, 512.0, 256.0] {
                v.span = [span; 2];
                let atlas = cache.terrain_atlas(&v, 1);
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
        fn detailed_biomes_survive_unload_refresh_and_stay_in_their_dimension_and_layer() {
            let mut cache = Cache::default();
            let mut tile = coarse_tile([90, 80, 70]);
            tile.biomes = vec![Some(Arc::from("minecraft:forest")); 256];
            tile.biomes[15] = Some(Arc::from("minecraft:desert"));
            cache.insert(Dimension::Overworld, Layer::Surface, [-1, -1], tile);
            let before = cache.coverage();
            // There is no live game chunk behind this lookup. Positions use
            // Euclidean chunk coordinates, including negative boundaries.
            assert_eq!(
                before
                    .detailed_biome(Dimension::Overworld, Layer::Surface, [-1, -16])
                    .as_deref(),
                Some("minecraft:desert")
            );
            assert_eq!(
                before
                    .detailed_biome(Dimension::Overworld, Layer::Surface, [-16, -16])
                    .as_deref(),
                Some("minecraft:forest")
            );
            assert!(
                before
                    .detailed_biome(Dimension::Nether, Layer::Surface, [-1, -16])
                    .is_none()
            );
            assert!(
                before
                    .detailed_biome(Dimension::Overworld, Layer::Below(32), [-1, -16])
                    .is_none()
            );
            assert!(
                before
                    .detailed_biome(Dimension::Overworld, Layer::Surface, [0, -16])
                    .is_none()
            );
            cache.insert(
                Dimension::Overworld,
                Layer::Surface,
                [-1, -1],
                coarse_tile([20, 30, 40]),
            );
            assert_eq!(
                cache
                    .coverage()
                    .detailed_biome(Dimension::Overworld, Layer::Surface, [-1, -16])
                    .as_deref(),
                Some("minecraft:desert")
            );
            let mut changed = coarse_tile([20, 30, 40]);
            changed.biomes = vec![Some(Arc::from("minecraft:plains")); 256];
            cache.insert(Dimension::Overworld, Layer::Surface, [-1, -1], changed);
            assert_eq!(
                cache
                    .coverage()
                    .detailed_biome(Dimension::Overworld, Layer::Surface, [-1, -16])
                    .as_deref(),
                Some("minecraft:plains")
            );
            assert_eq!(
                before
                    .detailed_biome(Dimension::Overworld, Layer::Surface, [-1, -16])
                    .as_deref(),
                Some("minecraft:desert")
            );
            assert!(
                Cache::default()
                    .coverage()
                    .detailed_biome(Dimension::Overworld, Layer::Surface, [-1, -16])
                    .is_none()
            );
        }
        #[test]
        fn biome_sampling_uses_the_visible_water_surface_and_selected_cave_layer() {
            let blocks = |_: i32, y: i32, _: i32| Block {
                rgb: if y > 40 { 0x3366aa } else { 0x888888 },
                alpha: if y > 40 { 120 } else { 0 },
                water: y > 40,
                ..Block::default()
            };
            let names = |at: [i32; 3]| {
                Some(Arc::from(if at[1] > 40 {
                    "minecraft:ocean"
                } else {
                    "minecraft:lush_caves"
                }))
            };
            let surface = sample_with_biomes(blocks, |_, _| 64, [-1, 2], 0, Layer::Surface, names);
            assert_eq!(surface.heights[0], 40);
            assert!(
                surface
                    .biomes
                    .iter()
                    .all(|name| name.as_deref() == Some("minecraft:ocean"))
            );
            let cave = sample_with_biomes(blocks, |_, _| 64, [-1, 2], 0, Layer::Below(32), names);
            assert!(
                cave.biomes
                    .iter()
                    .all(|name| name.as_deref() == Some("minecraft:lush_caves"))
            );
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
            assert_eq!(settings.labels().len(), 17);
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
