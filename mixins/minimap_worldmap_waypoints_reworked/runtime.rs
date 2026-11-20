// SPDX-License-Identifier: GPL-3.0-only
// Based on horizon-rust/horizon-native-mod-template (MIT); see LICENSE-MIT and NOTICE.
// Shared by Cargo and the injected game module. Test bodies live in *_tests.rs;
// macros expand them here because Mixins requires self-contained module payloads.

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

/// Opt-in bounded CPU timing. The terrain worker emits summaries, never the
/// render callback. Enable before launching with MWWR_PROFILE=1.
pub mod profiling {
    use std::{
        collections::BTreeMap,
        sync::{Mutex, OnceLock},
        time::{Duration, Instant},
    };
    struct Samples {
        micros: [u64; 128],
        count: usize,
        items: u64,
        bytes: u64,
    }
    impl Default for Samples {
        fn default() -> Self {
            Self {
                micros: [0; 128],
                count: 0,
                items: 0,
                bytes: 0,
            }
        }
    }
    static ENABLED: OnceLock<bool> = OnceLock::new();
    static SAMPLES: OnceLock<Mutex<BTreeMap<&'static str, Samples>>> = OnceLock::new();
    pub fn enabled() -> bool {
        *ENABLED.get_or_init(|| std::env::var_os("MWWR_PROFILE").is_some_and(|v| v == "1"))
    }
    pub struct Timer {
        name: &'static str,
        start: Option<Instant>,
    }
    pub fn measure(name: &'static str) -> Timer {
        Timer {
            name,
            start: enabled().then(Instant::now),
        }
    }
    impl Drop for Timer {
        fn drop(&mut self) {
            if let Some(start) = self.start {
                record(self.name, start.elapsed(), 0, 0);
            }
        }
    }
    pub fn record(name: &'static str, elapsed: Duration, items: u64, bytes: u64) {
        if !enabled() {
            return;
        }
        let mut all = SAMPLES
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let samples = all.entry(name).or_default();
        samples.micros[samples.count % 128] = elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
        samples.count += 1;
        samples.items = samples.items.saturating_add(items);
        samples.bytes = samples.bytes.saturating_add(bytes);
    }
    pub fn report() {
        let Some(all) = SAMPLES.get() else {
            return;
        };
        let snapshot = std::mem::take(&mut *all.lock().unwrap_or_else(|e| e.into_inner()));
        for (name, samples) in snapshot {
            let mut values = samples.micros[..samples.count.min(128)].to_vec();
            values.sort_unstable();
            let percentile =
                |percent: usize| values[(values.len() * percent).div_ceil(100).saturating_sub(1)];
            eprintln!(
                "[mwwr-profile] stage={name} count={} p95_us={} p99_us={} max_us={} items={} bytes={}",
                samples.count,
                percentile(95),
                percentile(99),
                values.last().unwrap(),
                samples.items,
                samples.bytes
            );
        }
    }
}

const SIZES: [u16; 7] = [64, 96, 128, 160, 192, 224, 256];

/// Stable SDK keys and the historical preference field each key controls.
pub const SDK_SETTING_ROWS: [(&str, usize); 18] = [
    ("enabled", 0),
    ("corner", 1),
    ("size", 2),
    ("shape", 3),
    ("zoom", 4),
    ("rotate", 5),
    ("mini_dimension", 7),
    ("world_dimension", 8),
    ("overworld_coordinates", 9),
    ("nether_coordinates", 10),
    ("death_waypoints", 11),
    ("death_waypoint_auto_remove", 12),
    ("world_waypoints", 13),
    ("world_waypoint_style", 14),
    ("directions", 15),
    ("direction_style", 16),
    ("labels_on_hover", 17),
    ("waypoint_letters", 20),
];
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

/// Row-based lists retain smooth wheel deltas until a full row is reached.
#[derive(Default)]
pub struct ListScroll {
    row: usize,
    wheel: f64,
}
impl ListScroll {
    pub fn offset(&self, max: usize) -> usize {
        self.row.min(max)
    }
    pub fn jump(&mut self, row: usize, max: usize) {
        self.row = row.min(max);
        self.wheel = 0.;
    }
    pub fn page(&mut self, forwards: bool, rows: usize, max: usize) {
        let row = self.offset(max);
        self.jump(
            if forwards {
                row.saturating_add(rows)
            } else {
                row.saturating_sub(rows)
            },
            max,
        );
    }
    pub fn scroll(&mut self, delta: f64, max: usize) {
        if !delta.is_finite() {
            return;
        }
        self.row = self.offset(max);
        self.wheel -= delta.clamp(-(max as f64 + 1.), max as f64 + 1.);
        let steps = (self.wheel.abs() + 1e-9).floor().copysign(self.wheel);
        self.wheel -= steps;
        self.row = if steps >= 0. {
            self.row.saturating_add(steps as usize).min(max)
        } else {
            self.row.saturating_sub((-steps) as usize)
        };
        if (self.row == 0 && self.wheel < 0.) || (self.row == max && self.wheel > 0.) {
            self.wheel = 0.;
        }
    }
}

/// Geometry shared by drawing, track clicks and thumb dragging.
pub struct ListScrollbar {
    pub thumb_top: f32,
    pub thumb_height: f32,
    pub max_row: usize,
    top: f32,
    travel: f32,
}
impl ListScrollbar {
    pub fn new(top: f32, height: f32, visible: usize, count: usize, row: usize) -> Self {
        let height = height.max(0.);
        let max_row = count.saturating_sub(visible);
        let thumb_height = if max_row == 0 {
            height
        } else {
            (height * visible as f32 / count as f32)
                .max(12.)
                .min(height)
        };
        let travel = height - thumb_height;
        let thumb_top = top + travel * row.min(max_row) as f32 / max_row.max(1) as f32;
        Self {
            thumb_top,
            thumb_height,
            max_row,
            top,
            travel,
        }
    }
    pub fn row_at(&self, mouse_y: f32, grab: f32) -> usize {
        if self.travel <= 0. || !mouse_y.is_finite() || !grab.is_finite() {
            return 0;
        }
        let ratio = ((mouse_y - self.top - grab) / self.travel).clamp(0., 1.);
        (ratio * self.max_row as f32).round() as usize
    }
}

// Keep diagnostic identities in English for format detection and logs. Translate
// at the UI boundary, so changing the game language also updates worker notices.
const MESSAGES: &[(&str, [&str; 4])] = &[
    (
        "Settings worker stopped",
        [
            "Поток настроек остановлен",
            "Le traitement des réglages est arrêté",
            "El proceso de ajustes se ha detenido",
            "Die Einstellungsverarbeitung wurde beendet",
        ],
    ),
    (
        "Settings file exceeds 16 MiB",
        [
            "Файл настроек превышает 16 МиБ",
            "Le fichier des réglages dépasse 16 Mio",
            "El archivo de ajustes supera los 16 MiB",
            "Die Einstellungsdatei überschreitet 16 MiB",
        ],
    ),
    (
        "No settings directory",
        [
            "Папка настроек недоступна",
            "Dossier des réglages indisponible",
            "La carpeta de ajustes no está disponible",
            "Einstellungsordner nicht verfügbar",
        ],
    ),
    (
        "Save",
        ["Сохранение", "Enregistrement", "Guardado", "Speichern"],
    ),
    (
        "Cache catalog worker failed",
        [
            "Не удалось загрузить список кешей",
            "Impossible de charger la liste des caches",
            "No se pudo cargar la lista de cachés",
            "Die Cache-Liste konnte nicht geladen werden",
        ],
    ),
    (
        "Map sampling failed; sampling paused",
        [
            "Ошибка сбора данных карты; сбор приостановлен",
            "Échec de la collecte de la carte ; collecte suspendue",
            "Error al recopilar el mapa; recopilación en pausa",
            "Kartenerfassung fehlgeschlagen; Erfassung angehalten",
        ],
    ),
    (
        "Map worker unavailable; sampling paused",
        [
            "Поток карты недоступен; сбор данных приостановлен",
            "Traitement de la carte indisponible ; collecte suspendue",
            "El proceso del mapa no está disponible; recopilación en pausa",
            "Kartenverarbeitung nicht verfügbar; Erfassung angehalten",
        ],
    ),
    (
        "Cache write failed; sampling paused",
        [
            "Ошибка записи кеша; сбор данных приостановлен",
            "Échec d’écriture du cache ; collecte suspendue",
            "Error al escribir la caché; recopilación en pausa",
            "Cache-Schreibfehler; Erfassung angehalten",
        ],
    ),
    (
        "Cannot open cache; sampling paused",
        [
            "Не удалось открыть кеш; сбор данных приостановлен",
            "Impossible d’ouvrir le cache ; collecte suspendue",
            "No se pudo abrir la caché; recopilación en pausa",
            "Cache kann nicht geöffnet werden; Erfassung angehalten",
        ],
    ),
    (
        "Unknown world identity; temporary RAM map only",
        [
            "Мир не определён; карта временно хранится только в памяти",
            "Monde inconnu ; carte temporaire en mémoire uniquement",
            "Mundo desconocido; mapa temporal solo en memoria",
            "Welt unbekannt; temporäre Karte nur im Arbeitsspeicher",
        ],
    ),
    (
        "Cache is used by another client; temporary RAM map only",
        [
            "Кеш занят другим клиентом; карта временно хранится только в памяти",
            "Cache utilisé par un autre client ; carte temporaire en mémoire uniquement",
            "Otro cliente usa la caché; mapa temporal solo en memoria",
            "Cache von anderem Client verwendet; temporäre Karte nur im Arbeitsspeicher",
        ],
    ),
    (
        "Open this cache's world before rebuilding its legacy file",
        [
            "Для обновления старого файла откройте мир этого кеша",
            "Ouvrez le monde de ce cache avant de reconstruire son ancien fichier",
            "Abre el mundo de esta caché antes de reconstruir su archivo antiguo",
            "Vor dem Neuaufbau der alten Datei die zugehörige Welt öffnen",
        ],
    ),
    (
        "Cache writer is paused; resolve the write error first",
        [
            "Запись кеша приостановлена; сначала устраните ошибку записи",
            "Écriture du cache suspendue ; corrigez d’abord l’erreur d’écriture",
            "La escritura de caché está en pausa; resuelve primero el error",
            "Cache-Schreiben angehalten; zuerst den Schreibfehler beheben",
        ],
    ),
    (
        "Cache selection changed; refresh the list",
        [
            "Выбранный кеш изменился; обновите список",
            "Le cache sélectionné a changé ; actualisez la liste",
            "La caché seleccionada ha cambiado; actualiza la lista",
            "Ausgewählter Cache hat sich geändert; Liste aktualisieren",
        ],
    ),
    (
        "Cache directory changed; refresh the list",
        [
            "Папка кешей изменилась; обновите список",
            "Le dossier des caches a changé ; actualisez la liste",
            "La carpeta de cachés ha cambiado; actualiza la lista",
            "Cache-Ordner hat sich geändert; Liste aktualisieren",
        ],
    ),
    (
        "Cache is in use by another client",
        [
            "Кеш занят другим клиентом",
            "Cache utilisé par un autre client",
            "Otro cliente está usando la caché",
            "Cache wird von einem anderen Client verwendet",
        ],
    ),
    (
        "Persistent cache is unavailable",
        [
            "Сохранение кеша на диске недоступно",
            "Cache persistant indisponible",
            "La caché persistente no está disponible",
            "Dauerhafter Cache nicht verfügbar",
        ],
    ),
    (
        "Unknown cache identity",
        [
            "Кеш не удалось определить",
            "Identité du cache inconnue",
            "Identidad de caché desconocida",
            "Cache-Identität unbekannt",
        ],
    ),
    (
        "Cache worker is unavailable",
        [
            "Поток кеша недоступен",
            "Traitement du cache indisponible",
            "El proceso de caché no está disponible",
            "Cache-Verarbeitung nicht verfügbar",
        ],
    ),
    (
        "Damaged cache record skipped",
        [
            "Повреждённая запись кеша пропущена",
            "Entrée endommagée du cache ignorée",
            "Se ha omitido un registro de caché dañado",
            "Beschädigter Cache-Eintrag übersprungen",
        ],
    ),
    (
        "invalid world identity",
        [
            "Некорректный идентификатор мира",
            "Identifiant du monde invalide",
            "Identificador de mundo no válido",
            "Ungültige Weltkennung",
        ],
    ),
    (
        "truncated container header",
        [
            "Заголовок кеша обрезан",
            "En-tête du cache tronqué",
            "La cabecera de caché está truncada",
            "Cache-Kopf abgeschnitten",
        ],
    ),
    (
        "no recoverable container header",
        [
            "Заголовок кеша не поддаётся восстановлению",
            "Aucun en-tête de cache récupérable",
            "No hay una cabecera de caché recuperable",
            "Kein wiederherstellbarer Cache-Kopf",
        ],
    ),
    (
        "unsupported container version",
        [
            "Неподдерживаемая версия формата кеша",
            "Version du format de cache non prise en charge",
            "Versión del formato de caché no compatible",
            "Nicht unterstützte Cache-Formatversion",
        ],
    ),
    (
        "world identity mismatch",
        [
            "Кеш принадлежит другому миру",
            "Le cache appartient à un autre monde",
            "La caché pertenece a otro mundo",
            "Cache gehört zu einer anderen Welt",
        ],
    ),
    (
        "no recoverable container root",
        [
            "Индекс кеша не поддаётся восстановлению",
            "Aucun index de cache récupérable",
            "No hay un índice de caché recuperable",
            "Kein wiederherstellbarer Cache-Index",
        ],
    ),
    (
        "zero generation",
        [
            "Некорректное поколение кеша",
            "Génération du cache invalide",
            "Generación de caché no válida",
            "Ungültige Cache-Generation",
        ],
    ),
    (
        "invalid index count",
        [
            "Некорректный размер индекса кеша",
            "Nombre d’entrées du cache invalide",
            "Número de entradas de caché no válido",
            "Ungültige Anzahl von Cache-Einträgen",
        ],
    ),
    (
        "invalid index extent",
        [
            "Повреждены границы индекса кеша",
            "Étendue de l’index du cache invalide",
            "Extensión del índice de caché no válida",
            "Ungültiger Bereich des Cache-Index",
        ],
    ),
    (
        "invalid index page",
        [
            "Повреждена страница индекса кеша",
            "Page de l’index du cache invalide",
            "Página del índice de caché no válida",
            "Ungültige Cache-Indexseite",
        ],
    ),
    (
        "invalid index page size",
        [
            "Некорректный размер страницы индекса кеша",
            "Taille de page d’index invalide",
            "Tamaño de página de índice no válido",
            "Ungültige Größe der Indexseite",
        ],
    ),
    (
        "unsorted index",
        [
            "Нарушен порядок индекса кеша",
            "Index du cache désordonné",
            "El índice de caché está desordenado",
            "Cache-Index nicht sortiert",
        ],
    ),
    (
        "invalid record extent",
        [
            "Повреждены границы записи кеша",
            "Étendue d’une entrée du cache invalide",
            "Extensión de registro de caché no válida",
            "Ungültiger Bereich des Cache-Eintrags",
        ],
    ),
    (
        "incomplete index",
        [
            "Индекс кеша неполный",
            "Index du cache incomplet",
            "El índice de caché está incompleto",
            "Cache-Index unvollständig",
        ],
    ),
    (
        "overlapping extents",
        [
            "Области кеша пересекаются",
            "Des régions du cache se chevauchent",
            "Hay regiones de caché superpuestas",
            "Cache-Bereiche überlappen sich",
        ],
    ),
    (
        "record checksum mismatch",
        [
            "Контрольная сумма записи кеша не совпадает",
            "Somme de contrôle de l’entrée du cache incorrecte",
            "La suma de comprobación del registro no coincide",
            "Prüfsumme des Cache-Eintrags stimmt nicht überein",
        ],
    ),
    (
        "invalid record length",
        [
            "Некорректная длина записи кеша",
            "Longueur d’entrée du cache invalide",
            "Longitud de registro de caché no válida",
            "Ungültige Länge des Cache-Eintrags",
        ],
    ),
    (
        "invalid terrain tile",
        [
            "Повреждён фрагмент карты",
            "Tuile de terrain invalide",
            "Fragmento de terreno no válido",
            "Ungültige Geländekachel",
        ],
    ),
    (
        "invalid terrain record",
        [
            "Повреждена запись рельефа",
            "Entrée de terrain invalide",
            "Registro de terreno no válido",
            "Ungültiger Geländeeintrag",
        ],
    ),
    (
        "invalid biome name",
        [
            "Некорректное название биома",
            "Nom de biome invalide",
            "Nombre de bioma no válido",
            "Ungültiger Biomname",
        ],
    ),
    (
        "record version overflow",
        [
            "Превышен предел версий записи",
            "Limite des versions d’entrée atteinte",
            "Se ha alcanzado el límite de versiones de registro",
            "Versionslimit des Eintrags erreicht",
        ],
    ),
    (
        "generation overflow",
        [
            "Превышен предел поколений кеша",
            "Limite des générations du cache atteinte",
            "Se ha alcanzado el límite de generaciones de caché",
            "Generationslimit des Cache erreicht",
        ],
    ),
    (
        "visual generation overflow",
        [
            "Превышен предел версий изображений",
            "Limite des versions d’images atteinte",
            "Se ha alcanzado el límite de versiones de imágenes",
            "Versionslimit der Bilder erreicht",
        ],
    ),
    (
        "allocation length overflow",
        [
            "Превышен допустимый размер кеша",
            "Taille maximale du cache dépassée",
            "Se ha superado el tamaño máximo de caché",
            "Maximale Cache-Größe überschritten",
        ],
    ),
    (
        "file offset overflow",
        [
            "Превышен допустимый размер файла",
            "Taille maximale du fichier dépassée",
            "Se ha superado el tamaño máximo del archivo",
            "Maximale Dateigröße überschritten",
        ],
    ),
    (
        "bad free-space view",
        [
            "Повреждена таблица свободного места кеша",
            "Table d’espace libre du cache invalide",
            "La tabla de espacio libre de caché no es válida",
            "Freispeichertabelle des Cache beschädigt",
        ],
    ),
    (
        "writer needs recovery after an I/O failure",
        [
            "Кеш заблокирован после ошибки записи",
            "Cache bloqué après un échec d’écriture",
            "La caché está bloqueada tras un error de escritura",
            "Cache nach Schreibfehler gesperrt",
        ],
    ),
    (
        "No space left on device",
        [
            "На диске недостаточно места",
            "Espace disque insuffisant",
            "No queda espacio en el disco",
            "Nicht genügend Speicherplatz auf dem Datenträger",
        ],
    ),
    (
        "Permission denied",
        [
            "Нет прав доступа к файлу",
            "Accès au fichier refusé",
            "Acceso al archivo denegado",
            "Dateizugriff verweigert",
        ],
    ),
    (
        "Access is denied.",
        [
            "Нет прав доступа к файлу",
            "Accès au fichier refusé",
            "Acceso al archivo denegado",
            "Dateizugriff verweigert",
        ],
    ),
    (
        "Read-only file system",
        [
            "Файловая система доступна только для чтения",
            "Système de fichiers en lecture seule",
            "El sistema de archivos es de solo lectura",
            "Dateisystem ist schreibgeschützt",
        ],
    ),
    (
        "No such file or directory",
        [
            "Файл или папка не найдены",
            "Fichier ou dossier introuvable",
            "No se encuentra el archivo o la carpeta",
            "Datei oder Ordner nicht gefunden",
        ],
    ),
    (
        "The system cannot find the file specified.",
        [
            "Файл не найден",
            "Fichier introuvable",
            "No se encuentra el archivo",
            "Datei nicht gefunden",
        ],
    ),
    (
        "Input/output error",
        [
            "Ошибка чтения или записи диска",
            "Erreur de lecture ou d’écriture du disque",
            "Error de lectura o escritura del disco",
            "Fehler beim Lesen oder Schreiben des Datenträgers",
        ],
    ),
    (
        "failed to fill whole buffer",
        [
            "Файл кеша обрезан",
            "Fichier de cache tronqué",
            "El archivo de caché está truncado",
            "Cache-Datei abgeschnitten",
        ],
    ),
];

pub fn message_for(language: Language, raw: &str) -> String {
    if language == Language::English {
        return raw.into();
    }
    let translate = |key: &str| {
        MESSAGES
            .iter()
            .find(|(en, _)| *en == key)
            .map(|(_, texts)| texts[language as usize - 1])
    };
    if let Some(text) = translate(raw) {
        return text.into();
    }
    if let Some((prefix, detail)) = raw.split_once(": ")
        && let Some(prefix) = translate(prefix)
    {
        return format!("{prefix}: {}", message_for(language, detail));
    }
    if let Some((detail, _)) = raw.rsplit_once(" (os error ")
        && let Some(text) = translate(detail)
    {
        return text.into();
    }
    [
        "Не удалось выполнить операцию",
        "Impossible d’effectuer l’opération",
        "No se pudo completar la operación",
        "Der Vorgang konnte nicht abgeschlossen werden",
    ][language as usize - 1]
        .into()
}

pub fn localize_message(raw: &str) -> String {
    message_for(language(), raw)
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

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub enum WaypointLetters {
        One,
        #[default]
        SimilarColors,
        AnyColors,
    }
    impl WaypointLetters {
        pub const ALL: [Self; 3] = [Self::One, Self::SimilarColors, Self::AnyColors];
        pub fn name(self) -> &'static str {
            match self {
                Self::One => "one",
                Self::SimilarColors => "similar",
                Self::AnyColors => "any",
            }
        }
        fn from_name(value: &str) -> Option<Self> {
            match value {
                "one" => Some(Self::One),
                "similar" => Some(Self::SimilarColors),
                "any" => Some(Self::AnyColors),
                _ => None,
            }
        }
        pub fn label(self) -> &'static str {
            match self {
                Self::One => tr(
                    "One letter",
                    "Одна буква",
                    "Une lettre",
                    "Una letra",
                    "Ein Buchstabe",
                ),
                Self::SimilarColors => tr(
                    "Two for similar colours",
                    "Две при похожих цветах",
                    "Deux si couleurs proches",
                    "Dos con colores similares",
                    "Zwei bei ähnlichen Farben",
                ),
                Self::AnyColors => tr(
                    "Two regardless of colour",
                    "Две независимо от цвета",
                    "Deux quelle que soit la couleur",
                    "Dos sin importar el color",
                    "Zwei unabhängig von der Farbe",
                ),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Settings {
        pub waypoint_letters: WaypointLetters,
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
        /// Draw a waypoint's 3D label (distance, or name and distance) only while
        /// the crosshair is on its marker, instead of always.
        pub labels_on_hover: bool,
    }

    impl Default for Settings {
        fn default() -> Self {
            Self {
                waypoint_letters: WaypointLetters::default(),
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
                labels_on_hover: true,
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
                    "waypoint_letters" => {
                        if let Some(mode) = WaypointLetters::from_name(raw) {
                            value.waypoint_letters = mode;
                        }
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
                    "labels_on_hover" => {
                        value.labels_on_hover = raw.parse().unwrap_or(value.labels_on_hover)
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
                "# Minimap Reworked settings\nenabled={}\ncorner={}\nsize={}\nshape={}\nzoom={}\nrotate={}\nmini_dimension={}\nworld_dimension={}\noverworld_coordinates={}\nnether_coordinates={}\ndeath_waypoints={}\ndeath_waypoint_auto_remove={}\nworld_waypoints={}\nworld_waypoint_style={}\ndirections={}\ndirection_style={}\nlabels_on_hover={}\ncache_ram_mib={}\nwaypoint_letters={}\n",
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
                self.labels_on_hover,
                self.cache_ram_mib,
                self.waypoint_letters.name(),
            )
        }

        /// Ordered choices for the SDK settings page, relative to this mod's
        /// historical defaults. Actions and the RAM slider have no choices.
        pub fn options(button: usize) -> Vec<Self> {
            if matches!(button, 6 | 18 | 19) || button > 20 {
                return Vec::new();
            }
            let initial = Self::default();
            let mut value = initial;
            let mut choices = Vec::new();
            loop {
                choices.push(value);
                if value.click(button) == 0 || choices.len() > 32 {
                    return Vec::new();
                }
                if value == initial {
                    return choices;
                }
            }
        }

        pub fn option_index(self, button: usize) -> Option<usize> {
            if button >= 32 {
                return None;
            }
            let mut selected = Self::default();
            selected.overlay(self, 1 << button);
            Self::options(button).iter().position(|v| *v == selected)
        }

        pub fn select_option(&mut self, button: usize, index: usize) -> bool {
            let Some(selected) = Self::options(button).get(index).copied() else {
                return false;
            };
            self.overlay(selected, 1 << button);
            true
        }

        pub fn option_label(self, button: usize) -> Option<String> {
            if button == 20 {
                return Some(self.waypoint_letters.label().into());
            }
            self.labels().get(button).map(|label| {
                label
                    .split_once(':')
                    .map_or(label.as_str(), |(_, v)| v.trim())
                    .to_owned()
            })
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
                    return 0x7ffbf | (1 << 20);
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
                17 => self.labels_on_hover = !self.labels_on_hover,
                20 => {
                    self.waypoint_letters =
                        super::cycle(self.waypoint_letters, &WaypointLetters::ALL)
                }
                _ => return 0,
            }
            1 << button
        }

        // Only locally edited fields override settings loaded asynchronously.
        pub(super) fn overlay(&mut self, edited: Self, mask: u32) {
            if mask & (1 << 20) != 0 {
                self.waypoint_letters = edited.waypoint_letters;
            }
            if mask & (1 << 18) != 0 {
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
            if mask & 131072 != 0 {
                self.labels_on_hover = edited.labels_on_hover;
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
                localized_format!(
                    "3D labels only on hover: {}",
                    "3D-подписи только при наведении: {}",
                    "Étiquettes 3D au survol uniquement : {}",
                    "Etiquetas 3D solo al apuntar: {}",
                    "3D-Beschriftungen nur beim Zielen: {}",
                    on(self.labels_on_hover)
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

/// A deliberately small RGB neighbourhood, independent of alpha. Labels do
/// not depend on camera/distance, so they stay consistent on maps and in 3D.
pub fn similar_waypoint_colors(a: u32, b: u32) -> bool {
    [16, 8, 0]
        .into_iter()
        .map(|shift| {
            let delta = ((a >> shift) & 255) as i32 - ((b >> shift) & 255) as i32;
            delta * delta
        })
        .sum::<i32>()
        <= 48 * 48
}
pub fn waypoint_labels(points: &[Waypoint], mode: minimap::WaypointLetters) -> Vec<String> {
    let initials: Vec<_> = points
        .iter()
        .map(|point| {
            let mut chars = point
                .name
                .chars()
                .filter(|c| !c.is_whitespace())
                .map(|c| c.to_uppercase().next().unwrap_or(c));
            (chars.next().unwrap_or('?'), chars.next())
        })
        .collect();
    let mut second = vec![false; points.len()];
    if mode != minimap::WaypointLetters::One {
        for (i, a) in points.iter().enumerate() {
            if a.icon != WaypointIcon::Letter {
                continue;
            }
            for (j, b) in points.iter().enumerate().skip(i + 1) {
                if b.icon == WaypointIcon::Letter
                    && a.world == b.world
                    && a.dimension == b.dimension
                    && initials[i].0 == initials[j].0
                    && (mode == minimap::WaypointLetters::AnyColors
                        || similar_waypoint_colors(a.color, b.color))
                {
                    second[i] = true;
                    second[j] = true;
                }
            }
        }
    }
    initials
        .into_iter()
        .zip(second)
        .map(|((first, next), expand)| {
            let mut out = first.to_string();
            if expand && let Some(next) = next {
                out.push(next);
            }
            out
        })
        .collect()
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenRequest {
    Map,
    Waypoints,
}

/// Match the SDK's registered bindings, including mouse mappings and modifiers.
/// Layout keys never replace physical bindings, as on Horizon's own controls.
pub fn bound_screen_request(
    key: &str,
    modifiers: u32,
    map: Option<(&str, u32)>,
    waypoints: Option<(&str, u32)>,
) -> Option<ScreenRequest> {
    [
        (map, ScreenRequest::Map),
        (waypoints, ScreenRequest::Waypoints),
    ]
    .into_iter()
    .find_map(|(binding, request)| {
        binding
            .filter(|(bound, bits)| *bound == key && bits & 7 == modifiers & 7)
            .map(|_| request)
    })
}

/// Public SDK callbacks publish invalidations without retaining a borrowed Host.
/// Native rendering consumes them before the next draw or sampling pass.
pub struct ClientEvents(AtomicU8);
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PendingClientEvents {
    pub disconnected: bool,
    pub resources_reloaded: bool,
}
impl ClientEvents {
    pub const fn new() -> Self {
        Self(AtomicU8::new(0))
    }
    pub fn disconnected(&self) {
        self.0.fetch_or(1, Ordering::Release);
    }
    pub fn resources_reloaded(&self) {
        self.0.fetch_or(2, Ordering::Release);
    }
    pub fn take(&self) -> PendingClientEvents {
        let events = self.0.swap(0, Ordering::AcqRel);
        PendingClientEvents {
            disconnected: events & 1 != 0,
            resources_reloaded: events & 2 != 0,
        }
    }
}
impl Default for ClientEvents {
    fn default() -> Self {
        Self::new()
    }
}
pub static CLIENT_EVENTS: ClientEvents = ClientEvents::new();

static SCREEN_REQUEST: AtomicU8 = AtomicU8::new(0);
static TEXT_INPUT: AtomicU8 = AtomicU8::new(0);
static WAYPOINT_BACKGROUND: AtomicU8 = AtomicU8::new(0);

pub fn set_waypoint_background_transparent(transparent: bool) {
    WAYPOINT_BACKGROUND.store(u8::from(transparent), Ordering::Release);
}
pub fn waypoint_background_transparent() -> bool {
    WAYPOINT_BACKGROUND.load(Ordering::Acquire) != 0
}

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
    loaded: bool,
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
    pub caches: cache_manager::Service,
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
            maps: maps::Service::with_directory(Some(cache_directory.clone())),
            caches: cache_manager::Service::new(cache_directory),
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
        labels.push(
            tr(
                "Cache manager… (in world)",
                "Менеджер кеша… (в мире)",
                "Gestion du cache… (en jeu)",
                "Gestor de caché… (en el mundo)",
                "Cache-Verwaltung… (im Spiel)",
            )
            .into(),
        );
        labels.push(localized_format!(
            "Waypoint initials: {}",
            "Буквы вейпоинтов: {}",
            "Initiales des repères : {}",
            "Iniciales de marcadores: {}",
            "Wegpunktinitialen: {}",
            state.settings.waypoint_letters.label()
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

    /// Apply one typed SDK choice without overwriting unrelated preferences
    /// that the asynchronous loader or a map control changed.
    pub fn set_option(&self, button: usize, index: usize) -> bool {
        let Some(selected) = minimap::Settings::options(button).get(index).copied() else {
            return false;
        };
        let notify = self.notify.lock().unwrap_or_else(|e| e.into_inner());
        let Some(notify) = notify.as_ref() else {
            return false;
        };
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let previous = state.settings;
        state.settings.overlay(selected, 1 << button);
        state.touched |= 1 << button;
        if state.settings != previous || !state.loaded {
            state.revision += 1;
        }
        drop(state);
        let _ = notify.try_send(());
        true
    }

    pub fn set_cache_ram_mib(&self, value: u16) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let value = value.clamp(16, 512);
        if state.settings.cache_ram_mib == value && state.loaded {
            return;
        }
        state.settings.cache_ram_mib = value;
        state.touched |= 1 << 18;
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
        self.caches.shutdown();
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
        state.loaded = true;
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
super::minimap_settings_tests::define_tests!();

// Shared in-memory terrain repository; retention is temporarily uncapped until the container.
// No game objects cross the worker boundary.
/// Checked single-file terrain storage, owned exclusively by the map worker.
/// Live writers own an exclusive lease; offline previews use shared leases.
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

    impl std::fmt::Display for LeaseError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Busy => f.write_str("Cache is in use by another client"),
                Self::Io(error) => error.fmt(f),
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct FileId {
        volume: u64,
        index: u64,
        created: Option<std::time::SystemTime>,
    }

    #[cfg(unix)]
    fn file_id(file: &File) -> io::Result<FileId> {
        use std::os::unix::fs::MetadataExt;
        let meta = file.metadata()?;
        Ok(FileId {
            volume: meta.dev(),
            index: meta.ino(),
            created: meta.created().ok(),
        })
    }

    #[cfg(windows)]
    fn file_id(file: &File) -> io::Result<FileId> {
        use std::os::windows::io::AsRawHandle;
        #[repr(C)]
        struct FileInformation {
            attributes: u32,
            created: [u32; 2],
            accessed: [u32; 2],
            written: [u32; 2],
            volume: u32,
            size_hi: u32,
            size_lo: u32,
            links: u32,
            index_hi: u32,
            index_lo: u32,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetFileInformationByHandle(
                file: std::os::windows::io::RawHandle,
                info: *mut FileInformation,
            ) -> i32;
        }
        let mut info = std::mem::MaybeUninit::uninit();
        // The borrowed File owns a valid handle. The API initializes the
        // entire repr(C) output on success.
        let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let info = unsafe { info.assume_init() };
        Ok(FileId {
            volume: info.volume.into(),
            index: (u64::from(info.index_hi) << 32) | u64::from(info.index_lo),
            created: file.metadata()?.created().ok(),
        })
    }

    #[cfg(not(any(unix, windows)))]
    fn file_id(_file: &File) -> io::Result<FileId> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Cache identity is unavailable",
        ))
    }

    pub(super) fn directory_handle(path: &Path) -> io::Result<File> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(0x0200_0000); // FILE_FLAG_BACKUP_SEMANTICS
        }
        let file = options.open(path)?;
        if !file.metadata()?.is_dir() {
            return Err(invalid("Cache directory changed; refresh the list"));
        }
        Ok(file)
    }

    fn lock_result(error: TryLockError) -> LeaseError {
        match error {
            TryLockError::WouldBlock => LeaseError::Busy,
            TryLockError::Error(error) => LeaseError::Io(error),
        }
    }

    /// Stored beside the cache directory so renaming/replacing that directory
    /// doesn't produce a second lock inode. Never remove or rename this file.
    struct NamespaceGate {
        lock: File,
    }

    fn namespace_parent(directory: &Path) -> &Path {
        directory
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
    }

    impl NamespaceGate {
        fn acquire(directory: &Path) -> Result<Self, LeaseError> {
            let name = directory
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            let (hash, _) = cache_hash_pair(&name);
            let path =
                namespace_parent(directory).join(format!(".mwwr-namespace-{hash:016x}.lock"));
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)
                .map_err(LeaseError::Io)?;
            lock.try_lock().map_err(lock_result)?;
            Ok(Self { lock })
        }
    }

    impl Drop for NamespaceGate {
        fn drop(&mut self) {
            // Release before an inherited descriptor from a concurrent
            // subprocess can prolong this short namespace operation.
            let _ = self.lock.unlock();
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct DeleteSelection {
        pub(super) file: String,
        directory_id: FileId,
        file_id: FileId,
        len: u64,
        modified: Option<std::time::SystemTime>,
    }

    impl DeleteSelection {
        #[cfg(test)]
        pub(super) fn read(directory: &Path, name: &str) -> io::Result<Self> {
            let parent = directory_handle(directory)?;
            let path = super::cache_manager::selected_path(directory, name)?;
            let file = File::open(path)?;
            let selection = Self::from_handles(name, &parent, &file)?;
            if file_id(&directory_handle(directory)?)? != selection.directory_id {
                return Err(invalid("Cache directory changed; refresh the list"));
            }
            Ok(selection)
        }

        pub(super) fn from_handles(name: &str, directory: &File, file: &File) -> io::Result<Self> {
            let metadata = file.metadata()?;
            Ok(Self {
                file: name.to_owned(),
                directory_id: file_id(directory)?,
                file_id: file_id(file)?,
                len: metadata.len(),
                modified: metadata.modified().ok(),
            })
        }
    }

    /// Detach the verified file under the namespace gate, then remove only the
    /// detached object. If an external rename races our checks, never unlink
    /// the replacement. An unrestorable detached object is kept for recovery.
    pub(super) fn delete_selected(
        directory: &Path,
        selection: &DeleteSelection,
    ) -> Result<(), LeaseError> {
        let _gate = NamespaceGate::acquire(directory)?;
        delete_in_namespace(directory, selection).map_err(LeaseError::Io)
    }

    fn delete_in_namespace(directory: &Path, selection: &DeleteSelection) -> io::Result<()> {
        let parent = directory_handle(directory)?;
        let changed = || invalid("Cache selection changed; refresh the list");
        if file_id(&parent)? != selection.directory_id {
            return Err(changed());
        }
        let path = super::cache_manager::selected_path(directory, &selection.file)?;
        let lease = ContainerLease::lock_existing(&path, false).map_err(|error| match error {
            LeaseError::Busy => io::Error::new(io::ErrorKind::WouldBlock, error.to_string()),
            LeaseError::Io(error) => error,
        })?;
        if DeleteSelection::from_handles(&selection.file, &parent, &lease.file)? != *selection
            || file_id(&directory_handle(directory)?)? != selection.directory_id
        {
            return Err(changed());
        }
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let stage = loop {
            let index = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let stage = namespace_parent(directory)
                .join(format!(".mwwr-delete-{}-{index}", std::process::id()));
            match std::fs::create_dir(&stage) {
                Ok(()) => break stage,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };
        let detached = stage.join("cache.mwwr");
        let result = (|| {
            std::fs::rename(&path, &detached)?;
            if File::open(&detached).and_then(|f| file_id(&f)).ok() != Some(selection.file_id)
                || directory_handle(directory).and_then(|d| file_id(&d)).ok()
                    != Some(selection.directory_id)
            {
                return Err(changed());
            }
            std::fs::remove_file(&detached)
        })();
        if result.is_err()
            && detached.exists()
            && directory_handle(directory)
                .and_then(|dir| file_id(&dir))
                .ok()
                == Some(selection.directory_id)
        {
            // Restore without overwriting a file created at the old name.
            if std::fs::hard_link(&detached, &path).is_ok() {
                let _ = std::fs::remove_file(&detached);
            }
        }
        let _ = std::fs::remove_dir(&stage);
        result
    }

    /// Owns exactly one locked descriptor.
    #[derive(Debug)]
    pub struct ContainerLease {
        file: File,
    }

    impl ContainerLease {
        /// Call on an I/O worker. No bytes are read or changed before ownership.
        /// Unsupported locking is an error, never permission to continue unlocked.
        pub fn open_existing(path: &Path) -> Result<Self, LeaseError> {
            let directory = path
                .parent()
                .ok_or_else(|| LeaseError::Io(invalid("Invalid cache file")))?;
            let _gate = NamespaceGate::acquire(directory)?;
            Self::lock_existing(path, false)
        }

        fn lock_existing(path: &Path, shared: bool) -> Result<Self, LeaseError> {
            if !std::fs::symlink_metadata(path)
                .map_err(LeaseError::Io)?
                .is_file()
            {
                return Err(LeaseError::Io(invalid("Invalid cache file")));
            }
            let file = OpenOptions::new()
                .read(true)
                .write(!shared)
                .open(path)
                .map_err(LeaseError::Io)?;
            if shared {
                file.try_lock_shared()
            } else {
                file.try_lock()
            }
            .map_err(lock_result)?;
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
    pub const SCHEMA: u32 = 2;
    // Bump when derived image semantics change. Zero is the legacy spelling
    // of revision 1. Revision 2 changes LOD colour semantics, not raw TIL1 bytes.
    pub const VISUAL_REVISION: u32 = 2;
    pub const LOD_PALETTE_REVISION: i32 = 1;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Compatibility {
        Current,
        RebuildImages,
        Older(u32),
        Newer(u32),
        Damaged,
    }
    pub fn compatibility(schema: u32, visual: u32) -> Compatibility {
        if schema < SCHEMA {
            Compatibility::Older(schema)
        } else if schema > SCHEMA || visual.max(1) > VISUAL_REVISION {
            Compatibility::Newer(schema)
        } else if visual.max(1) < VISUAL_REVISION {
            Compatibility::RebuildImages
        } else {
            Compatibility::Current
        }
    }
    pub struct FileInfo {
        pub world: Option<String>,
        pub bytes: u64,
        pub compatibility: Compatibility,
    }
    /// Bounded read-only discovery: no index/payload loading, rename or lease.
    /// A concurrent commit may give a previous valid root, never a torn label.
    pub fn inspect(path: &Path) -> io::Result<FileInfo> {
        inspect_file(&mut File::open(path)?)
    }

    pub(super) fn inspect_file(file: &mut File) -> io::Result<FileInfo> {
        let bytes = file.metadata()?.len();
        let mut best = None;
        for slot in 0..2 {
            let mut header = vec![0; PAGE];
            if read_at(file, (slot * PAGE) as u64, &mut header).is_err()
                || !checked(&header)
                || !header.starts_with(b"MWWRC")
            {
                continue;
            }
            let schema = u32_at(&header, 8);
            let generation = u64_at(&header, 16);
            if generation == 0 || best.as_ref().is_some_and(|(g, _, _)| *g >= generation) {
                continue;
            }
            let world = if &header[..8] == b"MWWRC002" && schema == SCHEMA {
                let len = u32_at(&header, 40) as usize;
                (len > 0 && len <= 2048)
                    .then(|| {
                        std::str::from_utf8(&header[48..48 + len])
                            .ok()
                            .map(str::to_owned)
                    })
                    .flatten()
            } else {
                None
            };
            let state = if schema == SCHEMA && &header[..8] != b"MWWRC002" {
                Compatibility::Newer(schema)
            } else if schema == SCHEMA && world.is_none() {
                Compatibility::Damaged
            } else {
                compatibility(schema, u32_at(&header, 44))
            };
            let state = if state == Compatibility::Current && u32_at(&header, 12) & 1 != 0 {
                Compatibility::RebuildImages
            } else {
                state
            };
            best = Some((generation, world, state));
        }
        let (_, world, compatibility) = best.unwrap_or((0, None, Compatibility::Damaged));
        Ok(FileInfo {
            world,
            bytes,
            compatibility,
        })
    }
    pub type RecordKey = [i32; 6];
    #[derive(Clone, Copy, Debug)]
    struct RecordRef {
        offset: u64,
        len: u32,
        checksum: u32,
        version: u64,
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
        spatial: SpatialIndex,
        pending_versions: BTreeMap<RecordKey, u64>,
        next_version: u64,
        pub(super) visual_revision: u32,
        pub(super) upgrade_pending: bool,
        #[cfg(test)]
        pub(super) write_budget: Option<usize>,
    }
    /// Region columns support bounded rectangle lookup without walking history.
    /// This is derived metadata; schema 2 bytes and recovery remain unchanged.
    #[derive(Default)]
    struct SpatialBand {
        columns: BTreeMap<i32, BTreeMap<i32, std::collections::BTreeSet<[i32; 2]>>>,
    }
    #[derive(Default)]
    struct SpatialIndex {
        bands: BTreeMap<[i32; 4], SpatialBand>,
    }
    impl SpatialIndex {
        fn insert(&mut self, key: RecordKey) {
            let band = self
                .bands
                .entry([key[0], key[1], key[2], key[5]])
                .or_default();
            band.columns
                .entry(key[3].div_euclid(32))
                .or_default()
                .entry(key[4].div_euclid(32))
                .or_default()
                .insert([key[3], key[4]]);
        }
        fn query(
            &self,
            dimension: i32,
            kind: i32,
            layer: i32,
            rect: [i64; 4],
            limit: usize,
            level: Option<i32>,
        ) -> Vec<RecordKey> {
            let mut found = Vec::new();
            if rect[2] <= 0 || rect[3] <= 0 || limit == 0 {
                return found;
            }
            for (&[d, k, y, level], band) in self.bands.range(
                [dimension, kind, layer, level.unwrap_or(i32::MIN)]
                    ..=[dimension, kind, layer, level.unwrap_or(i32::MAX)],
            ) {
                let size = 16_i64
                    << if matches!(kind, 2..=4) {
                        level.clamp(0, 22)
                    } else {
                        0
                    };
                let lo = [rect[0].div_euclid(size), rect[1].div_euclid(size)];
                let hi = [
                    (rect[0] + rect[2] - 1).div_euclid(size),
                    (rect[1] + rect[3] - 1).div_euclid(size),
                ];
                let clamp = |n: i64| n.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
                for (_, column) in band
                    .columns
                    .range(clamp(lo[0].div_euclid(32))..=clamp(hi[0].div_euclid(32)))
                {
                    for (_, cells) in
                        column.range(clamp(lo[1].div_euclid(32))..=clamp(hi[1].div_euclid(32)))
                    {
                        for &[x, z] in cells {
                            if (lo[0]..=hi[0]).contains(&i64::from(x))
                                && (lo[1]..=hi[1]).contains(&i64::from(z))
                            {
                                found.push([d, k, y, x, z, level]);
                                if found.len() == limit {
                                    return found;
                                }
                            }
                        }
                    }
                }
            }
            found
        }
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
        // Name the file after the world's folder/address so it can be told apart
        // in `cache/`, with a short identity hash so two names that sanitize to
        // the same string never share one file. The exact identity is also
        // checked in each root: a hash collision never merges worlds.
        directory.join(cache_file_name(world))
    }
    /// The stable 128-bit identity hash: used for the readable name's suffix and
    /// to find caches written before files were named after their world.
    fn cache_hash_pair(world: &str) -> (u64, u64) {
        let mut a = 0xcbf29ce484222325u64;
        let mut b = 0x84222325cbf29ce4u64;
        for byte in world.bytes() {
            a = (a ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            b = (b ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        (a, b)
    }
    fn cache_file_name(world: &str) -> String {
        let (kind, id) = world.split_once(':').unwrap_or(("world", world));
        let readable = cache_component(id);
        let (a, _) = cache_hash_pair(world);
        if readable.is_empty() {
            format!("{kind}-{a:016x}.mwwr")
        } else {
            format!("{kind}-{readable}-{a:016x}.mwwr")
        }
    }
    /// Filesystem-safe form of a save folder or server address: keeps letters,
    /// digits, spaces and `-_.`, replaces control characters and the characters
    /// Windows rejects, trims trailing dots/spaces, and bounds the byte length so
    /// the whole name stays under the filesystem limit.
    fn cache_component(id: &str) -> String {
        let mut out = String::new();
        for c in id.chars() {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                out.push('_');
            } else {
                out.push(c);
            }
            if out.len() >= 80 {
                break;
            }
        }
        out.trim_matches(|c| c == '.' || c == ' ').to_string()
    }
    /// Before files were named after their world, a cache was
    /// `<128-bit-hash>.mwwr`. Rename such a file to the readable name so
    /// existing terrain is kept; a failed rename just starts a fresh cache.
    fn migrate_legacy_cache(
        directory: &Path,
        world: &str,
        target: &Path,
    ) -> Result<(), LeaseError> {
        if target.exists() {
            return Ok(());
        }
        let (a, b) = cache_hash_pair(world);
        let legacy = directory.join(format!("{a:016x}{b:016x}.mwwr"));
        match ContainerLease::lock_existing(&legacy, false) {
            Ok(_lease) => std::fs::rename(&legacy, target).map_err(LeaseError::Io),
            Err(LeaseError::Io(e)) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
    /// Best-effort migration of closed legacy containers. All name changes
    /// share the same gate as open/create and deletion.
    pub fn migrate_cache_names(directory: &Path) {
        let Ok(_gate) = NamespaceGate::acquire(directory) else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("mwwr") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if stem.len() != 32 || !stem.bytes().all(|b| b.is_ascii_hexdigit()) {
                continue;
            }
            let Ok(mut lease) = ContainerLease::lock_existing(&path, false) else {
                continue;
            };
            let Some(world) =
                read_world_identity(&mut lease.file).filter(|w| super::valid_waypoint_world(w))
            else {
                continue;
            };
            let target = world_path(directory, &world);
            if target != path && !target.exists() {
                let _ = std::fs::rename(&path, &target);
            }
        }
    }
    /// The world identity stored in a cache's first header, when it is a valid
    /// container header.
    fn read_world_identity(file: &mut File) -> Option<String> {
        let mut header = vec![0; PAGE];
        read_at(file, 0, &mut header).ok()?;
        if &header[..8] != b"MWWRC002" || !checked(&header) {
            return None;
        }
        let world_len = u32_at(&header, 40) as usize;
        if world_len == 0 || world_len > 2048 || 48 + world_len > header.len() {
            return None;
        }
        std::str::from_utf8(&header[48..48 + world_len])
            .ok()
            .map(str::to_owned)
    }
    impl Store {
        pub fn open(directory: &Path, world: &str) -> Result<Self, LeaseError> {
            if world.is_empty() || world.len() > 2048 {
                return Err(LeaseError::Io(invalid("invalid world identity")));
            }
            std::fs::create_dir_all(directory).map_err(LeaseError::Io)?;
            let gate = NamespaceGate::acquire(directory)?;
            let path = world_path(directory, world);
            migrate_legacy_cache(directory, world, &path)?;
            let lease = match ContainerLease::lock_existing(&path, false) {
                Ok(lease) => lease,
                Err(LeaseError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {
                    let file = OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create_new(true)
                        .open(&path)
                        .map_err(LeaseError::Io)?;
                    file.try_lock().map_err(lock_result)?;
                    ContainerLease { file }
                }
                Err(e) => return Err(e),
            };
            drop(gate);
            Self::from_lease(lease, world)
        }
        pub fn open_existing(path: &Path, world: &str) -> Result<Self, LeaseError> {
            if world.is_empty() || world.len() > 2048 {
                return Err(LeaseError::Io(invalid("invalid world identity")));
            }
            Self::from_existing(ContainerLease::open_existing(path)?, world)
        }
        pub(super) fn open_preview(path: &Path, world: &str) -> Result<Self, LeaseError> {
            let directory = path
                .parent()
                .ok_or_else(|| LeaseError::Io(invalid("Invalid cache file")))?;
            let gate = NamespaceGate::acquire(directory)?;
            let lease = ContainerLease::lock_existing(path, true)?;
            drop(gate);
            Self::from_existing(lease, world)
        }
        fn from_existing(mut lease: ContainerLease, world: &str) -> Result<Self, LeaseError> {
            if lease.file_len().map_err(LeaseError::Io)? < PREFIX {
                return Err(LeaseError::Io(invalid("truncated container header")));
            }
            let mut header = vec![0; PAGE];
            let mut initialized = false;
            for slot in 0..2 {
                read_at(&mut lease.file, (slot * PAGE) as u64, &mut header)
                    .map_err(LeaseError::Io)?;
                initialized |=
                    header.starts_with(b"MWWRC") && checked(&header) && u64_at(&header, 16) != 0;
            }
            if !initialized {
                return Err(LeaseError::Io(invalid("no recoverable container header")));
            }
            Self::from_lease(lease, world)
        }
        fn from_lease(lease: ContainerLease, world: &str) -> Result<Self, LeaseError> {
            let mut store = Self {
                lease,
                world: world.into(),
                roots: Default::default(),
                current: 0,
                pending: BTreeMap::new(),
                poisoned: false,
                spatial: SpatialIndex::default(),
                pending_versions: BTreeMap::new(),
                next_version: 0,
                visual_revision: VISUAL_REVISION,
                upgrade_pending: false,
                #[cfg(test)]
                write_budget: None,
            };
            store.load().map_err(LeaseError::Io)?;
            for key in store.roots[store.current].entries.keys() {
                store.spatial.insert(*key);
            }
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
                if checked(&header)
                    && header.starts_with(b"MWWRC")
                    && (&header[..8] != b"MWWRC002"
                        || u32_at(&header, 8) != SCHEMA
                        || u32_at(&header, 44).max(1) > VISUAL_REVISION
                        || u32_at(&header, 12) & !1 != 0)
                {
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
            let mut header = vec![0; PAGE];
            read_at(
                &mut self.lease.file,
                (self.current * PAGE) as u64,
                &mut header,
            )?;
            self.visual_revision = u32_at(&header, 44).max(1);
            self.upgrade_pending =
                u32_at(&header, 12) & 1 != 0 || self.visual_revision < VISUAL_REVISION;
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
                        version: 0,
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
            let _timing = super::profiling::measure("disk-read");
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
            self.next_version = self
                .next_version
                .checked_add(1)
                .ok_or_else(|| invalid("record version overflow"))?;
            self.pending.insert(key, bytes);
            self.pending_versions.insert(key, self.next_version);
            self.spatial.insert(key);
            Ok(())
        }
        pub fn query(
            &self,
            dimension: i32,
            kind: i32,
            layer: i32,
            rect: [i64; 4],
            limit: usize,
        ) -> Vec<RecordKey> {
            self.query_sources(dimension, kind, layer, rect, limit, None)
        }
        pub fn query_level(
            &self,
            dimension: i32,
            kind: i32,
            layer: i32,
            level: u8,
            rect: [i64; 4],
            limit: usize,
        ) -> Vec<RecordKey> {
            self.query_sources(dimension, kind, layer, rect, limit, Some(i32::from(level)))
        }
        fn query_sources(
            &self,
            dimension: i32,
            kind: i32,
            layer: i32,
            rect: [i64; 4],
            limit: usize,
            level: Option<i32>,
        ) -> Vec<RecordKey> {
            if kind != 2 || layer != 0 {
                return self
                    .spatial
                    .query(dimension, kind, layer, rect, limit, level);
            }
            let mut out = Vec::new();
            let mut seen = std::collections::BTreeSet::new();
            for revision in (0..=LOD_PALETTE_REVISION).rev() {
                for key in self
                    .spatial
                    .query(dimension, kind, revision, rect, limit, level)
                {
                    if seen.insert((key[3], key[4], key[5])) {
                        out.push(key);
                    }
                    if out.len() == limit {
                        return out;
                    }
                }
            }
            out
        }
        pub fn source_keys_after(&self, after: Option<RecordKey>, limit: usize) -> Vec<RecordKey> {
            use std::ops::Bound::{Excluded, Included};
            let mut keys = std::collections::BTreeSet::new();
            for d in 1..=3 {
                for kind in 0..=2 {
                    let low = [d, kind, i32::MIN, i32::MIN, i32::MIN, i32::MIN];
                    let high = [d, kind, i32::MAX, i32::MAX, i32::MAX, i32::MAX];
                    if after.is_some_and(|last| last >= high) {
                        continue;
                    }
                    let start = after
                        .filter(|last| *last >= low)
                        .map_or(Included(low), Excluded);
                    keys.extend(
                        self.roots[self.current]
                            .entries
                            .range((start, Included(high)))
                            .map(|(k, _)| *k)
                            .take(limit),
                    );
                    keys.extend(
                        self.pending
                            .range((start, Included(high)))
                            .map(|(k, _)| *k)
                            .take(limit),
                    );
                    if keys.len() >= limit {
                        return keys.into_iter().take(limit).collect();
                    }
                }
            }
            keys.into_iter().take(limit).collect()
        }
        pub fn version(&self, key: RecordKey) -> Option<u64> {
            self.pending_versions.get(&key).copied().or_else(|| {
                self.roots[self.current]
                    .entries
                    .get(&key)
                    .map(|r| r.version)
            })
        }
        pub fn pending_bytes(&self) -> usize {
            self.pending.values().map(Vec::len).sum()
        }
        pub fn dirty(&self) -> bool {
            !self.pending.is_empty()
        }
        pub fn generation(&self) -> u64 {
            self.roots[self.current].generation
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
            let _timing = super::profiling::measure("commit");
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
                        version: self.pending_versions[&key],
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
            put32(&mut header, 12, u32::from(self.upgrade_pending));
            put64(&mut header, 16, generation);
            put64(&mut header, 24, root.pages.first().map_or(0, |e| e.start));
            put64(&mut header, 32, root.entries.len() as u64);
            put32(&mut header, 40, self.world.len() as u32);
            put32(&mut header, 44, VISUAL_REVISION);
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
            self.visual_revision = VISUAL_REVISION;
            self.pending.clear();
            self.pending_versions.clear();
            Ok(())
        }
    }
    #[cfg(test)]
    super::super::minimap_container_tests::define_tests!();
}

/// The catalog is deliberately independent of the map file owner. It reads only
/// headers; rebuilding is submitted to the existing terrain worker instead.
pub mod cache_manager {
    use super::*;
    pub const MAX_FILES: usize = 4096;
    #[derive(Clone)]
    pub struct Entry {
        pub file: String,
        pub deletion: Option<container::DeleteSelection>,
        pub world: Option<String>,
        pub name: String,
        pub icon: Option<Arc<Vec<u8>>>, // validated 64x64 RGBA, never a source path
        pub bytes: u64,
        pub compatibility: container::Compatibility,
    }
    #[derive(Clone, Default)]
    pub struct Snapshot {
        pub entries: Vec<Entry>,
        pub loading: bool,
        pub error: Option<String>,
        pub truncated: bool,
        pub revision: u64,
    }
    type Resolve = Box<dyn FnOnce(&mut [Entry]) -> io::Result<()> + Send>;
    pub struct Service {
        state: Arc<Mutex<Arc<Snapshot>>>,
        tx: Mutex<Option<mpsc::SyncSender<Resolve>>>,
        worker: Mutex<Option<JoinHandle<()>>>,
    }
    pub fn selected_path(directory: &Path, file: &str) -> io::Result<PathBuf> {
        let name = Path::new(file);
        if file.contains(['/', '\\', ':'])
            || name.file_name().and_then(|name| name.to_str()) != Some(file)
            || name.extension().is_none_or(|ext| ext != "mwwr")
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid cache file",
            ));
        }
        let path = directory.join(name);
        if !fs::symlink_metadata(&path)?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid cache file",
            ));
        }
        Ok(path)
    }

    pub fn scan(directory: &Path) -> io::Result<Snapshot> {
        let files = match fs::read_dir(directory) {
            Ok(files) => files,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Snapshot::default()),
            Err(e) => return Err(e),
        };
        let directory = container::directory_handle(directory).ok();
        let mut out = Snapshot::default();
        for file in files {
            let file = file?;
            if !file.file_type()?.is_file() || file.path().extension().is_none_or(|e| e != "mwwr") {
                continue;
            }
            if out.entries.len() == MAX_FILES {
                out.truncated = true;
                break;
            }
            let path = file.path();
            let Ok(name) = file.file_name().into_string() else {
                continue;
            };
            let mut handle = fs::File::open(&path).ok();
            let info = handle
                .as_mut()
                .and_then(|file| container::inspect_file(file).ok())
                .unwrap_or_else(|| container::FileInfo {
                    world: None,
                    bytes: file.metadata().map_or(0, |m| m.len()),
                    compatibility: container::Compatibility::Damaged,
                });
            let deletion = directory
                .as_ref()
                .zip(handle.as_ref())
                .and_then(|(directory, file)| {
                    container::DeleteSelection::from_handles(&name, directory, file).ok()
                });
            out.entries.push(Entry {
                name: info
                    .world
                    .as_deref()
                    .and_then(|w| w.split_once(':'))
                    .map_or_else(|| name.clone(), |(_, n)| n.to_owned()),
                deletion,
                file: name,
                world: info.world,
                icon: None,
                bytes: info.bytes,
                compatibility: info.compatibility,
            });
        }
        out.entries.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.file.cmp(&b.file))
        });
        Ok(out)
    }
    impl Service {
        pub fn new(directory: PathBuf) -> Self {
            let state = Arc::new(Mutex::new(Arc::new(Snapshot::default())));
            let shared = state.clone();
            let (tx, rx) = mpsc::sync_channel::<Resolve>(1);
            let worker = thread::Builder::new()
                .name("map-catalog".into())
                .spawn(move || {
                    while let Ok(resolve) = rx.recv() {
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let mut out = scan(&directory)?;
                            resolve(&mut out.entries)?;
                            out.entries.sort_by(|a, b| {
                                a.name
                                    .to_lowercase()
                                    .cmp(&b.name.to_lowercase())
                                    .then(a.file.cmp(&b.file))
                            });
                            Ok::<_, io::Error>(out)
                        }));
                        let mut slot = shared.lock().unwrap_or_else(|e| e.into_inner());
                        let revision = slot.revision + 1;
                        let mut out = match result {
                            Ok(Ok(out)) => out,
                            other => {
                                let mut out = (**slot).clone();
                                out.loading = false;
                                out.error = Some(match other {
                                    Ok(Err(e)) => e.to_string(),
                                    _ => "Cache catalog worker failed".into(),
                                });
                                out
                            }
                        };
                        out.revision = revision;
                        *slot = Arc::new(out);
                    }
                });
            match worker {
                Ok(worker) => Self {
                    state,
                    tx: Mutex::new(Some(tx)),
                    worker: Mutex::new(Some(worker)),
                },
                Err(e) => {
                    *state.lock().unwrap_or_else(|e| e.into_inner()) = Arc::new(Snapshot {
                        error: Some(e.to_string()),
                        ..Snapshot::default()
                    });
                    Self {
                        state,
                        tx: Mutex::new(None),
                        worker: Mutex::new(None),
                    }
                }
            }
        }
        pub fn snapshot(&self) -> Arc<Snapshot> {
            self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }
        pub fn refresh(
            &self,
            resolve: impl FnOnce(&mut [Entry]) -> io::Result<()> + Send + 'static,
        ) -> bool {
            let tx = self.tx.lock().unwrap_or_else(|e| e.into_inner());
            let Some(tx) = tx.as_ref() else {
                return false;
            };
            let mut slot = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if slot.loading {
                return false;
            }
            if tx.try_send(Box::new(resolve)).is_err() {
                return false;
            }
            let mut next = (**slot).clone();
            next.loading = true;
            next.error = None;
            *slot = Arc::new(next);
            true
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
    pub fn lod_water_color(
        base: Option<u32>,
        water: u32,
        depth: u32,
        freezes: bool,
        ice: u32,
    ) -> u32 {
        if freezes {
            return ice;
        }
        base.map_or(water, |base| blend(base, water, water_alpha(depth)))
    }
    /// LOD may omit thin weather layers. Only natural exposed surfaces get an
    /// approximate cover; actual snow/ice, buildings and warm/dry biomes keep
    /// their supplied block colour. Canopies retain visible foliage beneath it.
    pub fn lod_snow_color(name: &str, color: u32, snow: u32, snows: bool) -> u32 {
        if !snows {
            return color;
        }
        if matches!(
            name,
            "minecraft:oak_leaves"
                | "minecraft:spruce_leaves"
                | "minecraft:birch_leaves"
                | "minecraft:jungle_leaves"
                | "minecraft:acacia_leaves"
                | "minecraft:dark_oak_leaves"
                | "minecraft:mangrove_leaves"
                | "minecraft:cherry_leaves"
                | "minecraft:pale_oak_leaves"
        ) {
            blend(color, snow, 96)
        } else if matches!(
            name,
            "minecraft:grass_block"
                | "minecraft:podzol"
                | "minecraft:dirt"
                | "minecraft:coarse_dirt"
                | "minecraft:rooted_dirt"
                | "minecraft:mycelium"
                | "minecraft:moss_block"
                | "minecraft:stone"
                | "minecraft:andesite"
                | "minecraft:diorite"
                | "minecraft:granite"
                | "minecraft:gravel"
                | "minecraft:sand"
        ) {
            snow
        } else {
            color
        }
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
        /// Bound lookahead so a remote drag/teleport cannot displace the visible
        /// working set. Padding also retains terrain behind the moving camera.
        pub fn predictive(&self, target: &Self) -> Self {
            let mut prepared = self.clone();
            if (self.session, self.dimension, self.layer, self.large)
                != (target.session, target.dimension, target.layer, target.large)
            {
                return prepared;
            }
            for axis in 0..2 {
                let shift = (target.center[axis] - self.center[axis])
                    .clamp(-self.span[axis] / 4., self.span[axis] / 4.);
                prepared.center[axis] += shift / 2.;
                prepared.span[axis] = self.span[axis]
                    .max(target.span[axis].clamp(self.span[axis] / 2., self.span[axis] * 1.25))
                    + shift.abs();
            }
            prepared
        }
        fn visible_region(&self) -> [i64; 4] {
            let extents = if self.large {
                self.span
            } else {
                [self.span[0].hypot(self.span[1]); 2]
            };
            let left = (self.center[0] - extents[0] / 2.).floor() as i64;
            let top = (self.center[1] - extents[1] / 2.).floor() as i64;
            [
                left,
                top,
                (self.center[0] + extents[0] / 2.).ceil() as i64 - left,
                (self.center[1] + extents[1] / 2.).ceil() as i64 - top,
            ]
        }
        fn atlas_region(&self) -> [i64; 4] {
            let step = 2f64.powf(
                (self.span[0].max(self.span[1]) / 4.0)
                    .max(64.0)
                    .log2()
                    .ceil(),
            );
            let origin = self.center.map(|v| (v / step).round() * step);
            let extent = if self.large {
                self.span
            } else {
                [self.span[0].hypot(self.span[1]); 2]
            };
            let radius = extent.map(|v| (v / 2.0 / step).ceil() * step + step);
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
    const MAX_ZOOM: f64 = 512.0;

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
    #[cfg(test)]
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
        pub slots: Vec<usize>,
        sources: BTreeMap<VisualKey, [VisualSource; 3]>,
        locations: BTreeMap<VisualKey, usize>,
        scope: (u64, Dimension, Layer),
        pub prepared_tiles: usize,
        pub pages: Vec<Arc<AtlasPage>>,
        pub draw_order: Vec<usize>,
    }
    pub struct AtlasPage {
        pub pixels: Vec<u8>,
        pub width: usize,
        pub height: usize,
    }
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum VisualKey {
        #[cfg(test)]
        Detail([i32; 2]),
        Lod(LodKey),
    }
    #[derive(Clone)]
    enum VisualSource {
        Stored(Option<u64>),
        Memory(std::sync::Weak<Tile>),
        Image(std::sync::Weak<Overview>, Vec<(LodKey, VisualSource)>),
    }
    struct ShadeRequest {
        tile: Arc<Tile>,
        north: Option<Arc<Tile>>,
        west: Option<Arc<Tile>>,
        reply: mpsc::SyncSender<Vec<u8>>,
    }
    /// A bounded CPU stage. Only the terrain worker holds file descriptors;
    /// immutable neighbouring tiles cross this queue, never the container.
    struct Shading {
        sender: Option<mpsc::SyncSender<ShadeRequest>>,
        worker: Option<JoinHandle<()>>,
    }
    impl Shading {
        fn new() -> Option<Self> {
            let (sender, jobs) = mpsc::sync_channel::<ShadeRequest>(8);
            let worker = thread::Builder::new()
                .name("map-shading".into())
                .spawn(move || {
                    while let Ok(job) = jobs.recv() {
                        let _timing = super::profiling::measure("shade-worker");
                        let pixels = Cache::shade_pixels(
                            &job.tile,
                            job.north.as_deref(),
                            job.west.as_deref(),
                        );
                        let _ = job.reply.send(pixels);
                    }
                })
                .map_err(|e| {
                    eprintln!("Map shading worker unavailable; using inline preparation: {e}")
                })
                .ok()?;
            Some(Self {
                sender: Some(sender),
                worker: Some(worker),
            })
        }
        fn submit(
            &self,
            tile: Arc<Tile>,
            north: Option<Arc<Tile>>,
            west: Option<Arc<Tile>>,
        ) -> Option<mpsc::Receiver<Vec<u8>>> {
            let (reply, result) = mpsc::sync_channel(1);
            self.sender
                .as_ref()?
                .send(ShadeRequest {
                    tile,
                    north,
                    west,
                    reply,
                })
                .ok()?;
            Some(result)
        }
    }
    impl Drop for Shading {
        fn drop(&mut self) {
            self.sender.take();
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }
    impl VisualSource {
        fn same(&self, other: &Self) -> bool {
            match (self, other) {
                (Self::Stored(a), Self::Stored(b)) => a == b,
                (Self::Memory(a), Self::Memory(b)) => std::sync::Weak::ptr_eq(a, b),
                (Self::Image(a, sa), Self::Image(b, sb)) => {
                    std::sync::Weak::ptr_eq(a, b)
                        && sa.len() == sb.len()
                        && sa
                            .iter()
                            .zip(sb)
                            .all(|((ka, a), (kb, b))| ka == kb && a.same(b))
                }
                _ => false,
            }
        }
    }
    impl TerrainAtlas {
        pub fn covers(&self, view: &View) -> bool {
            let extent = if view.large {
                view.span
            } else {
                [view.span[0].hypot(view.span[1]); 2]
            };
            (0..2).all(|axis| {
                view.center[axis] - extent[axis] / 2.0 >= self.region[axis] as f64
                    && view.center[axis] + extent[axis] / 2.0
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
            let slot = self.slots[slot];
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
    /// Clip textured terrain in normalized map coordinates before emitting GPU geometry.
    /// The 64-sided mask differs from a true circle by less than 0.16 GUI pixels at size 256.
    pub fn terrain_polygon(view: &View, bounds: [i64; 4], uv: [f32; 4]) -> Vec<[f64; 4]> {
        let (sn, cs) = view.angle.sin_cos();
        let mut points: Vec<_> = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]]
            .into_iter()
            .map(|p| {
                let dx = bounds[0] as f64 + p[0] * bounds[2] as f64 - view.center[0];
                let dz = bounds[1] as f64 + p[1] * bounds[3] as f64 - view.center[1];
                [
                    (dx * cs + dz * sn) / view.span[0],
                    (-dx * sn + dz * cs) / view.span[1],
                    f64::from(uv[0]) + p[0] * f64::from(uv[2]),
                    f64::from(uv[1]) + p[1] * f64::from(uv[3]),
                ]
            })
            .collect();
        if (0..2).any(|axis| {
            points.iter().all(|p| p[axis] < -0.5) || points.iter().all(|p| p[axis] > 0.5)
        }) {
            return Vec::new();
        }
        if view.round && points.iter().all(|p| p[0] * p[0] + p[1] * p[1] < 0.249) {
            return points;
        }
        if !view.round
            && points
                .iter()
                .all(|p| p[0].abs() <= 0.5 && p[1].abs() <= 0.5)
        {
            return points;
        }
        static CIRCLE: std::sync::OnceLock<[[f64; 2]; 64]> = std::sync::OnceLock::new();
        let planes: &[[f64; 2]] = if view.round {
            CIRCLE.get_or_init(|| {
                std::array::from_fn(|edge| {
                    let (s, c) = ((edge as f64 + 0.5) * std::f64::consts::TAU / 64.).sin_cos();
                    [c, s]
                })
            })
        } else {
            &[[1., 0.], [0., 1.], [-1., 0.], [0., -1.]]
        };
        let limit = if view.round {
            0.5 * (std::f64::consts::PI / 64.).cos()
        } else {
            0.5
        };
        let mut out = Vec::with_capacity(12);
        for &[c, s] in planes {
            out.clear();
            if points.is_empty() {
                break;
            }
            let mut previous = *points.last().unwrap();
            let distance = |p: [f64; 4]| p[0] * c + p[1] * s - limit;
            let mut before = distance(previous);
            for &point in &points {
                let after = distance(point);
                if (before <= 0.) != (after <= 0.) {
                    let t = before / (before - after);
                    out.push(std::array::from_fn(|i| {
                        previous[i] + (point[i] - previous[i]) * t
                    }));
                }
                if after <= 0. {
                    out.push(point);
                }
                previous = point;
                before = after;
            }
            std::mem::swap(&mut points, &mut out);
        }
        points
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

    /// Place an off-screen indicator on the ray from the usable map center.
    pub fn edge_indicator(point: [f32; 2], bounds: [f32; 4]) -> Option<([f32; 2], [f32; 2])> {
        let [x, y, w, h] = bounds;
        if w <= 0. || h <= 0. || point.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let middle = [x + w / 2., y + h / 2.];
        let delta = [point[0] - middle[0], point[1] - middle[1]];
        let scale = (delta[0].abs() / (w / 2.)).max(delta[1].abs() / (h / 2.));
        if scale <= 1. {
            return None;
        }
        let length = delta[0].hypot(delta[1]);
        Some((
            [middle[0] + delta[0] / scale, middle[1] + delta[1] / scale],
            [delta[0] / length, delta[1] / length],
        ))
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

    /// Coalesce changed 16-row bands. Repacking is safe: comparison is of final
    /// pixels, so a slot move updates both its old and new image locations.
    pub fn image_patch_regions(
        previous: &[u8],
        next: &[u8],
        width: usize,
        height: usize,
    ) -> Vec<[usize; 4]> {
        if width == 0 || height == 0 || next.len() != width * height * 4 {
            return Vec::new();
        }
        if previous.len() != next.len() {
            return vec![[0, 0, width, height]];
        }
        let mut patches = Vec::new();
        let mut bytes = 0;
        for y in (0..height).step_by(16) {
            let rows = 16.min(height - y);
            let mut left = width;
            let mut right = 0;
            for x in (0..width).step_by(16) {
                let columns = 16.min(width - x);
                if (y..y + rows).any(|row| {
                    let start = (row * width + x) * 4;
                    previous[start..start + columns * 4] != next[start..start + columns * 4]
                }) {
                    left = left.min(x);
                    right = x + columns;
                }
            }
            if right > left {
                patches.push([left, y, right - left, rows]);
                bytes += (right - left) * rows * 4;
            }
        }
        if bytes > next.len() / 2 {
            vec![[0, 0, width, height]]
        } else {
            patches
        }
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
            container::LOD_PALETTE_REVISION,
            key.x,
            key.z,
            i32::from(key.level),
        ]
    }
    fn fill_lod_unknown(tile: &mut Tile, fallback: &Tile) {
        for i in 0..256 {
            if tile.pixels[i * 4 + 3] == 0 && fallback.pixels[i * 4 + 3] != 0 {
                tile.pixels[i * 4..i * 4 + 4].copy_from_slice(&fallback.pixels[i * 4..i * 4 + 4]);
                tile.heights[i] = fallback.heights[i];
                if let Some(biome) = fallback.biomes.get(i).cloned().flatten() {
                    tile.biomes.resize(256, None);
                    tile.biomes[i] = Some(biome);
                }
            }
        }
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
        read_order: BTreeSet<(u64, RecordKey)>,
        read_bytes: usize,
        clock: u64,
        limit: usize,
        error: Option<String>,
        unreadable: BTreeSet<RecordKey>,
    }
    impl Backing {
        fn get(&mut self, key: RecordKey) -> Option<Arc<Tile>> {
            if self.unreadable.contains(&key) {
                return None;
            }
            self.clock += 1;
            if let Some((stamp, tile)) = self.reads.get_mut(&key) {
                self.read_order.remove(&(*stamp, key));
                *stamp = self.clock;
                self.read_order.insert((*stamp, key));
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
                    self.unreadable.insert(key);
                    return None;
                }
            };
            let bytes = tile_bytes(&tile);
            while self.read_bytes + bytes > self.limit {
                let Some((_, key)) = self.read_order.pop_first() else {
                    break;
                };
                self.read_bytes -= tile_bytes(&self.reads.remove(&key).unwrap().1);
            }
            self.read_bytes += bytes;
            self.reads.insert(key, (self.clock, tile.clone()));
            self.read_order.insert((self.clock, key));
            Some(tile)
        }
    }
    type TileCursor = Option<((i32, i32), Option<Arc<Tile>>)>;
    type Key = (Dimension, Layer, i32, i32);
    const VISUAL_CHECKPOINT: RecordKey = [0, 7, 0, 0, 0, 0];
    const LAST_POSITION: RecordKey = [0, 8, 0, 0, 0, 0];

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct LastPosition {
        pub dimension: Dimension,
        pub block: [i32; 3],
        pub yaw: Option<i16>,
    }

    impl LastPosition {
        fn encode(self) -> Vec<u8> {
            let mut bytes: Vec<_> = [
                self.dimension as i32,
                self.block[0],
                self.block[1],
                self.block[2],
            ]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect();
            if let Some(yaw) = self.yaw {
                bytes.extend(yaw.to_le_bytes());
            }
            bytes
        }

        fn decode(bytes: &[u8]) -> Option<Self> {
            if !matches!(bytes.len(), 16 | 18) {
                return None;
            }
            let get = |i: usize| i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
            Some(Self {
                dimension: Dimension::ALL
                    .get(usize::try_from(get(0)).ok()?)
                    .copied()
                    .filter(|d| *d != Dimension::Player)?,
                block: [get(1), get(2), get(3)],
                yaw: bytes
                    .get(16..18)
                    .map(|yaw| i16::from_le_bytes([yaw[0], yaw[1]])),
            })
        }
    }

    #[derive(Clone)]
    pub struct CachedColumn {
        pub height: Option<i32>,
        pub biome: Option<Arc<str>>,
        pub approximate: bool,
    }

    pub fn map_coordinates(text: &str) -> Option<[i32; 2]> {
        let mut fields = text
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty());
        let x: i32 = fields.next()?.parse().ok()?;
        let z: i32 = fields.next()?.parse().ok()?;
        (fields.next().is_none()
            && [x, z]
                .into_iter()
                .all(|n| (-30_000_000..=30_000_000).contains(&n)))
        .then_some([x, z])
    }

    type PreviewPortals = Arc<Vec<([i32; 3], Portal)>>;

    #[derive(Clone, Debug, PartialEq)]
    pub struct PreviewDimension {
        pub dimension: Dimension,
        pub layers: Vec<(Layer, [f64; 2])>,
    }

    pub struct Preview {
        pub position: Option<LastPosition>,
        pub dimensions: Vec<PreviewDimension>,
        cache: Cache,
        atlas: Option<AtlasCache>,
        portal_region: Option<(Dimension, Layer, [i64; 4])>,
        portals: PreviewPortals,
        portal_work: std::collections::VecDeque<RecordKey>,
    }

    impl Preview {
        pub fn open(
            directory: &Path,
            file: &str,
            world: &str,
        ) -> Result<Self, container::LeaseError> {
            let path = super::cache_manager::selected_path(directory, file)
                .map_err(container::LeaseError::Io)?;
            let mut store = Store::open_preview(&path, world)?;
            let position = store
                .read(LAST_POSITION)
                .ok()
                .flatten()
                .and_then(|bytes| LastPosition::decode(&bytes));
            let mut dimensions = BTreeMap::<Dimension, BTreeMap<Layer, [f64; 2]>>::new();
            for key in store.keys().filter(|key| matches!(key[1], 0..=2)) {
                if let Some(dimension) = record_dimension(key)
                    && let Some(layer) = if key[1] == 2 {
                        Some(Layer::Surface)
                    } else {
                        record_layer(key)
                    }
                {
                    let size = 16_i64 << if key[1] == 2 { key[5].clamp(0, 22) } else { 0 };
                    dimensions
                        .entry(dimension)
                        .or_default()
                        .entry(layer)
                        .or_insert([
                            (i64::from(key[3]) * size + size / 2) as f64,
                            (i64::from(key[4]) * size + size / 2) as f64,
                        ]);
                }
            }
            let mut dimensions: Vec<_> = dimensions
                .into_iter()
                .map(|(dimension, layers)| {
                    let last = position.filter(|at| at.dimension == dimension);
                    let mut layers: Vec<_> = layers.into_iter().collect();
                    layers.sort_by_key(|(layer, _)| match layer {
                        Layer::Surface => (dimension == Dimension::Nether, 0),
                        Layer::Below(y) => (
                            dimension != Dimension::Nether,
                            y.abs_diff(last.map_or(64, |at| at.block[1].saturating_add(1))),
                        ),
                    });
                    if let Some(at) = last
                        && let Some((_, center)) = layers.first_mut()
                    {
                        *center = [f64::from(at.block[0]), f64::from(at.block[2])];
                    }
                    PreviewDimension { dimension, layers }
                })
                .collect();
            dimensions.sort_by_key(|d| position.is_none_or(|p| p.dimension != d.dimension));
            let mut cache = Cache::from_store(store, 32 * 1024 * 1024)?;
            cache.read_only = true;
            // Prepare only the current view and its target. Derived images
            // missing from the file are rebuilt in memory on demand.
            cache.overviews.done = true;
            Ok(Self {
                position,
                dimensions,
                cache,
                atlas: None,
                portal_region: None,
                portals: Arc::new(Vec::new()),
                portal_work: Default::default(),
            })
        }

        pub fn prepare(&mut self, view: &View, target: &View) -> io::Result<Arc<TerrainAtlas>> {
            self.cache.presentation_step(view, target, None)?;
            let atlas = self.cache.prepared_atlas(view, target, &mut self.atlas);
            self.cache.overviews.displayed |= !atlas.chunks.is_empty() || !atlas.coarse.is_empty();
            Ok(atlas)
        }

        pub fn portals(&mut self, view: &View) -> PreviewPortals {
            // Portal metadata lives in detailed tiles. Probe a bounded
            // neighbourhood on the worker; do not read tiles on the GUI thread.
            let [x, z] = view.center.map(|v| (v as i64).div_euclid(128) * 128);
            let region = [x - 128, z - 128, 256, 256];
            let scope = (view.dimension, view.layer, region);
            if self.portal_region != Some(scope) {
                self.portal_region = Some(scope);
                let kind = if view.layer == Layer::Surface { 0 } else { 1 };
                self.portal_work = self.cache.query_visual(view, 0, region, kind, 256).into();
                self.portals = Arc::new(Vec::new());
            }
            let start = Instant::now();
            for _ in 0..16 {
                if start.elapsed() >= Duration::from_millis(2) {
                    break;
                }
                let Some(key) = self.portal_work.pop_front() else {
                    break;
                };
                if let Some(tile) = self
                    .cache
                    .tile_at((view.dimension, view.layer, key[3], key[4]))
                    && !tile.portals.is_empty()
                {
                    Arc::make_mut(&mut self.portals).extend(tile.portals.iter().copied());
                }
            }
            self.portals.clone()
        }

        pub fn column(
            &self,
            dimension: Dimension,
            layer: Layer,
            at: [i32; 2],
        ) -> Option<CachedColumn> {
            let [x, z] = at.map(|v| v.div_euclid(16));
            if let Some(tile) = self.cache.tile_at((dimension, layer, x, z)) {
                let cell = (at[1].rem_euclid(16) * 16 + at[0].rem_euclid(16)) as usize;
                return Some(CachedColumn {
                    height: (tile.pixels[cell * 4 + 3] != 0).then_some(tile.heights[cell]),
                    biome: tile.biomes.get(cell).cloned().flatten(),
                    approximate: false,
                });
            }
            if layer != Layer::Surface {
                return None;
            }
            let mut levels = self.cache.lod_levels.get(&dimension).copied().unwrap_or(0);
            while levels != 0 {
                let level = levels.trailing_zeros() as u8;
                levels &= levels - 1;
                let size = 16_i64 << level;
                let key = LodKey {
                    x: i64::from(at[0]).div_euclid(size) as i32,
                    z: i64::from(at[1]).div_euclid(size) as i32,
                    level,
                };
                if let Some(tile) = self.cache.coarse_at(dimension, key)
                    && let Some(cell) = key.cell(at)
                    && tile.pixels[cell * 4 + 3] != 0
                {
                    return Some(CachedColumn {
                        height: Some(tile.heights[cell]),
                        biome: tile.biomes.get(cell).cloned().flatten(),
                        approximate: true,
                    });
                }
            }
            None
        }

        pub fn portals_pending(&self) -> bool {
            !self.portal_work.is_empty()
        }

        pub fn preparing(&self) -> bool {
            self.cache.visual_work()
        }
    }

    #[derive(Clone, PartialEq, Eq)]
    struct Overview {
        pixels: Vec<u8>,
        coverage: [u16; 256],
    }
    impl Default for Overview {
        fn default() -> Self {
            Self {
                pixels: vec![0; 1024],
                coverage: [0; 256],
            }
        }
    }
    impl Overview {
        fn encode(&self) -> Vec<u8> {
            let mut bytes = b"OVR1\x01\0\0\0".to_vec();
            bytes.extend_from_slice(&self.pixels);
            for value in self.coverage {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes
        }
        fn decode(bytes: &[u8]) -> Option<Self> {
            if bytes.len() != 1544 || &bytes[..8] != b"OVR1\x01\0\0\0" {
                return None;
            }
            Some(Self {
                pixels: bytes[8..1032].to_vec(),
                coverage: std::array::from_fn(|i| {
                    u16::from_le_bytes(bytes[1032 + i * 2..1034 + i * 2].try_into().unwrap())
                }),
            })
        }
        fn reduce(children: &[Option<Arc<Self>>; 4]) -> Self {
            let mut out = Self::default();
            for z in 0..16 {
                for x in 0..16 {
                    let Some(child) = &children[x / 8 + z / 8 * 2] else {
                        continue;
                    };
                    let mut alpha = 0u32;
                    let mut rgb = [0u32; 3];
                    let mut known = 0u32;
                    for dz in 0..2 {
                        for dx in 0..2 {
                            let at = (z % 8 * 2 + dz) * 16 + x % 8 * 2 + dx;
                            let a = u32::from(child.pixels[at * 4 + 3]);
                            alpha += a;
                            known += u32::from(child.coverage[at]);
                            for (c, value) in rgb.iter_mut().enumerate() {
                                *value += u32::from(child.pixels[at * 4 + c]) * a;
                            }
                        }
                    }
                    let at = z * 16 + x;
                    out.coverage[at] = if known == 0 {
                        0
                    } else {
                        ((known + 2) / 4).max(1) as u16
                    };
                    out.pixels[at * 4 + 3] = ((alpha + 2) / 4) as u8;
                    for (c, value) in rgb.iter().enumerate() {
                        out.pixels[at * 4 + c] =
                            (value + alpha / 2).checked_div(alpha).unwrap_or(0) as u8;
                    }
                }
            }
            out
        }
        fn fill(&mut self, at: usize, fallback: &[u8]) {
            self.fill_partial(at, fallback, if fallback[3] == 0 { 0 } else { u16::MAX });
        }
        fn fill_partial(&mut self, at: usize, fallback: &[u8], coverage: u16) {
            if self.coverage[at] == u16::MAX || coverage == 0 {
                return;
            }
            let missing = u64::from(u16::MAX - self.coverage[at]);
            let a = u64::from(self.pixels[at * 4 + 3]) * 65535;
            let b = u64::from(fallback[3]) * missing;
            let alpha = a + b;
            for (c, value) in fallback.iter().enumerate().take(3) {
                self.pixels[at * 4 + c] =
                    (u64::from(self.pixels[at * 4 + c]) * a + u64::from(*value) * b + alpha / 2)
                        .checked_div(alpha)
                        .unwrap_or(0) as u8;
            }
            self.pixels[at * 4 + 3] = ((alpha + 32767) / 65535) as u8;
            self.coverage[at] = (u64::from(self.coverage[at])
                + (u64::from(coverage) * missing + 32767) / 65535)
                .min(65535) as u16;
        }
    }
    fn overview_key(d: Dimension, layer: Layer, key: LodKey) -> RecordKey {
        let mut record = record_key(d, layer, key.x, key.z);
        record[1] += 3;
        record[5] = i32::from(key.level);
        record
    }
    fn overview_layer(key: RecordKey) -> Layer {
        if key[1] == 4 {
            Layer::Below(key[2])
        } else {
            Layer::Surface
        }
    }
    fn footprint(key: RecordKey) -> LodKey {
        LodKey {
            x: key[3],
            z: key[4],
            level: key[5] as u8,
        }
    }
    type PresentationGoal = (u64, Dimension, Layer, [i64; 4], u8, [i64; 4], u8);
    #[derive(Default)]
    struct Overviews {
        tiles: BTreeMap<RecordKey, (u64, Arc<Overview>)>,
        order: BTreeSet<(u64, RecordKey)>,
        clock: u64,
        dirty: BTreeSet<(u8, RecordKey)>,
        urgent: BTreeSet<(u8, RecordKey)>,
        fresh: BTreeSet<RecordKey>,
        stale: bool,
        cursor: Option<RecordKey>,
        done: bool,
        modified: bool,
        wanted: BTreeSet<RecordKey>,
        load_order: std::collections::VecDeque<RecordKey>,
        rebuild_order: std::collections::VecDeque<RecordKey>,
        goal: Option<PresentationGoal>,
        bad: BTreeSet<RecordKey>,
        displayed: bool,
    }
    impl Overviews {
        fn load(store: &mut Store) -> Self {
            let mut out = Self {
                stale: true,
                ..Self::default()
            };
            if store.visual_revision < container::VISUAL_REVISION {
                return out;
            }
            let Ok(Some(bytes)) = store.read(VISUAL_CHECKPOINT) else {
                return out;
            };
            if bytes.len() < 48
                || &bytes[..8] != b"OVC1\x01\0\0\0"
                || u64::from_le_bytes(bytes[8..16].try_into().unwrap()) != store.generation()
            {
                return out;
            }
            let count = u32::from_le_bytes(bytes[44..48].try_into().unwrap()) as usize;
            if count > 32768
                || bytes.len() != 48 + count * 24
                || bytes[17] != 0
                || bytes[16] > 1
                || bytes[18] > 1
            {
                return out;
            }
            let read_key = |bytes: &[u8]| {
                std::array::from_fn(|i| {
                    i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                })
            };
            out.stale = false;
            out.done = bytes[16] != 0;
            if bytes[18] != 0 {
                out.cursor = Some(read_key(&bytes[20..44]));
            }
            for chunk in bytes[48..].as_chunks::<24>().0 {
                let key: RecordKey = read_key(chunk);
                if !matches!(key[1], 3 | 4) || !(0..=22).contains(&key[5]) {
                    return Self {
                        stale: true,
                        ..Self::default()
                    };
                }
                out.dirty.insert((key[5] as u8, key));
            }
            out
        }
        fn checkpoint(&self, generation: u64) -> Vec<u8> {
            let restart = self.stale || self.dirty.len() > 32768;
            let mut bytes = b"OVC1\x01\0\0\0".to_vec();
            bytes.extend_from_slice(&generation.to_le_bytes());
            bytes.extend_from_slice(&[
                u8::from(self.done && !restart),
                u8::from(restart),
                u8::from(self.cursor.is_some() && !restart),
                0,
            ]);
            for n in if restart {
                [0; 6]
            } else {
                self.cursor.unwrap_or([0; 6])
            } {
                bytes.extend_from_slice(&n.to_le_bytes());
            }
            bytes.extend_from_slice(
                &(if restart { 0 } else { self.dirty.len() as u32 }).to_le_bytes(),
            );
            if !restart {
                for (_, key) in &self.dirty {
                    for n in key {
                        bytes.extend_from_slice(&n.to_le_bytes());
                    }
                }
            }
            bytes
        }
        fn mark(&mut self, source: RecordKey, urgent: bool) {
            if !(0..=2).contains(&source[1]) {
                return;
            }
            let base = if source[1] == 2 {
                source[5].clamp(0, 22) as u8
            } else {
                0
            };
            let mut key = source;
            key[1] = if source[1] == 1 { 4 } else { 3 };
            key[2] = if source[1] == 1 { source[2] } else { 0 };
            for level in base..=22 {
                key[5] = i32::from(level);
                self.dirty.insert((level, key));
                if urgent {
                    self.urgent.insert((level, key));
                }
                key[3] = key[3].div_euclid(2);
                key[4] = key[4].div_euclid(2);
            }
            self.modified = true;
        }
        fn remember(&mut self, key: RecordKey, tile: Arc<Overview>, capacity: usize) {
            if let Some((stamp, _)) = self.tiles.remove(&key) {
                self.order.remove(&(stamp, key));
            }
            self.clock += 1;
            self.tiles.insert(key, (self.clock, tile));
            if !self.wanted.contains(&key) {
                self.order.insert((self.clock, key));
            }
            let capacity = capacity
                .max(self.wanted.len().saturating_add(512))
                .min(65536);
            let mut attempts = self.tiles.len();
            while self.tiles.len() > capacity && attempts > 0 {
                attempts -= 1;
                let Some((_, old)) = self.order.pop_first() else {
                    break;
                };
                if self.wanted.contains(&old) {
                    continue;
                } else {
                    self.tiles.remove(&old);
                }
            }
        }
    }
    #[derive(Default)]
    pub struct Cache {
        read_only: bool,
        lod_current_colors: BTreeSet<(Dimension, LodKey)>,
        tiles: BTreeMap<Key, (u64, Arc<Tile>)>,
        lod_tiles: BTreeMap<(Dimension, LodKey), (u64, Arc<Tile>)>,
        clock: u64,
        backing: Option<RefCell<Backing>>,
        limit: usize,
        #[cfg(test)]
        paged: bool,
        lod_levels: BTreeMap<Dimension, u32>,
        overviews: Overviews,
    }
    impl Cache {
        fn merge_transient(&mut self, transient: &Cache) -> io::Result<()> {
            for (&(dimension, layer, x, z), (_, tile)) in &transient.tiles {
                self.insert(dimension, layer, [x, z], (**tile).clone());
                let tile = self.tiles[&(dimension, layer, x, z)].1.clone();
                self.save(record_key(dimension, layer, x, z), tile)?;
                self.trim()?;
            }
            for (&(dimension, key), (_, tile)) in &transient.lod_tiles {
                if self.insert_lod(dimension, key, (**tile).clone()) {
                    let tile = self.lod_tiles[&(dimension, key)].1.clone();
                    self.save(lod_record(dimension, key), tile)?;
                    self.trim()?;
                }
            }
            self.flush()
        }

        fn schedule_visual_rebuild(&mut self) -> io::Result<()> {
            self.flush()?;
            self.overviews = Overviews {
                stale: true,
                modified: true,
                ..Overviews::default()
            };
            self.clock += 1;
            if let Some(backing) = &self.backing {
                backing.borrow_mut().store.upgrade_pending = false;
            }
            self.flush()
        }
        fn open(
            directory: &Path,
            world: &str,
            limit: usize,
        ) -> Result<Self, container::LeaseError> {
            Self::from_store(Store::open(directory, world)?, limit)
        }
        fn from_store(mut store: Store, limit: usize) -> Result<Self, container::LeaseError> {
            let overviews = Overviews::load(&mut store);
            let mut lod_levels = BTreeMap::new();
            for key in store
                .keys()
                .filter(|key| key[1] == 2 && (0..=22).contains(&key[5]))
            {
                if let Some(dimension) = record_dimension(key) {
                    *lod_levels.entry(dimension).or_insert(0) |= 1 << key[5];
                }
            }
            Ok(Self {
                lod_levels,
                overviews,
                backing: Some(RefCell::new(Backing {
                    store,
                    reads: BTreeMap::new(),
                    read_order: BTreeSet::new(),
                    read_bytes: 0,
                    clock: 0,
                    limit: limit / 4,
                    error: None,
                    unreadable: BTreeSet::new(),
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
            if self.read_only {
                return Ok(());
            }
            if let Some(backing) = &self.backing {
                let mut backing = backing.borrow_mut();
                if backing.store.dirty() || self.overviews.modified {
                    let marker = self.overviews.checkpoint(
                        backing
                            .store
                            .generation()
                            .checked_add(1)
                            .ok_or_else(|| io::Error::other("visual generation overflow"))?,
                    );
                    backing.store.put(VISUAL_CHECKPOINT, marker)?;
                    backing.store.commit()?;
                    self.overviews.modified = false;
                }
            }
            Ok(())
        }
        fn save(&mut self, key: RecordKey, tile: Arc<Tile>) -> io::Result<()> {
            if key[1] == 2 {
                let visual = Overview {
                    pixels: tile.pixels.clone(),
                    coverage: std::array::from_fn(|i| {
                        if tile.pixels.get(i * 4 + 3).is_some_and(|a| *a != 0) {
                            u16::MAX
                        } else {
                            0
                        }
                    }),
                };
                self.overviews.bad.remove(&key);
                if let Some((_, d, layer, region, _, future, _)) = self.overviews.goal
                    && record_dimension(key) == Some(d)
                    && layer == Layer::Surface
                    && (intersects(region, footprint(key).bounds())
                        || intersects(future, footprint(key).bounds()))
                    && self.overviews.wanted.len() < 32768
                {
                    self.overviews.wanted.insert(key);
                }
                self.overviews.remember(key, Arc::new(visual), 32768);
            }
            self.overviews.mark(key, true);
            if key[1] != 2 {
                for (dx, dz) in [(1, 0), (0, 1)] {
                    let mut neighbour = key;
                    neighbour[3] = neighbour[3].saturating_add(dx);
                    neighbour[4] = neighbour[4].saturating_add(dz);
                    self.overviews.mark(neighbour, true);
                }
            }
            if let Some(backing) = &self.backing {
                let mut backing = backing.borrow_mut();
                if let Some((stamp, old)) = backing.reads.remove(&key) {
                    backing.read_order.remove(&(stamp, key));
                    backing.read_bytes -= tile_bytes(&old);
                }
                backing.store.put(key, tile_encode(&tile)?)?;
                backing.unreadable.remove(&key);
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
                        self.lod_current_colors.remove(&key);
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
            self.lod_current_colors.clear();
            self.tiles.clear();
            self.lod_tiles.clear();
            if let Some(b) = &self.backing {
                let mut b = b.borrow_mut();
                b.reads.clear();
                b.read_order.clear();
                b.read_bytes = 0;
            }
            self.clock += 1;
        }
        fn overview_get(&mut self, key: RecordKey) -> Option<Arc<Overview>> {
            if self.overviews.bad.contains(&key)
                || self.overviews.stale && key[1] != 2 && !self.overviews.fresh.contains(&key)
            {
                return None;
            }
            let capacity =
                ((self.limit / 2).clamp(32 * 1024 * 1024, 64 * 1024 * 1024) / 1680).max(1);
            if let Some((_, tile)) = self.overviews.tiles.get(&key) {
                let tile = tile.clone();
                self.overviews.remember(key, tile.clone(), capacity);
                return Some(tile);
            }
            if self.read_only && self.overviews.stale && key[1] != 2 {
                return None;
            }
            let data = self.backing.as_ref()?.borrow_mut().store.read(key);
            let tile = match data {
                Ok(None) => return None,
                Ok(Some(bytes)) => {
                    if key[1] == 2 {
                        tile_decode(&bytes).ok().map(|mut tile| {
                            if key[2] == container::LOD_PALETTE_REVISION
                                && tile.pixels.as_chunks::<4>().0.iter().any(|p| p[3] == 0)
                            {
                                let mut legacy = key;
                                legacy[2] = 0;
                                if let Some(old) = self
                                    .backing
                                    .as_ref()
                                    .and_then(|b| b.borrow_mut().get(legacy))
                                {
                                    fill_lod_unknown(&mut tile, &old);
                                }
                            }
                            Overview {
                                coverage: std::array::from_fn(|i| {
                                    if tile.pixels[i * 4 + 3] != 0 {
                                        u16::MAX
                                    } else {
                                        0
                                    }
                                }),
                                pixels: tile.pixels,
                            }
                        })
                    } else {
                        Overview::decode(&bytes)
                    }
                }
                Err(_) => None,
            };
            let Some(tile) = tile else {
                if key[1] == 2 {
                    self.overviews.bad.insert(key);
                    return None;
                }
                if self.read_only {
                    self.overviews.bad.insert(key);
                }
                self.overviews.dirty.insert((key[5] as u8, key));
                self.overviews.urgent.insert((key[5] as u8, key));
                self.overviews.modified = true;
                return None;
            };
            let tile = Arc::new(tile);
            self.overviews.remember(key, tile.clone(), capacity);
            self.clock += 1;
            Some(tile)
        }
        fn rebuild_overview(
            &mut self,
            key: RecordKey,
            shading: Option<&Shading>,
        ) -> io::Result<()> {
            let Some(d) = record_dimension(key) else {
                return Ok(());
            };
            let layer = overview_layer(key);
            let node = footprint(key);
            let mut image = if node.level == 0 {
                if let Some(tile) = self.tile_at((d, layer, node.x, node.z)) {
                    let pixels = shading
                        .and_then(|s| {
                            s.submit(
                                tile.clone(),
                                self.tile_at((d, layer, node.x, node.z.saturating_sub(1))),
                                self.tile_at((d, layer, node.x.saturating_sub(1), node.z)),
                            )
                        })
                        .and_then(|r| r.recv().ok())
                        .unwrap_or_else(|| self.shaded_chunk(d, layer, [node.x, node.z]));
                    Overview {
                        pixels,
                        coverage: [u16::MAX; 256],
                    }
                } else {
                    Overview::default()
                }
            } else {
                let children = std::array::from_fn(|i| {
                    let x = node.x.checked_mul(2)?.checked_add((i % 2) as i32)?;
                    let z = node.z.checked_mul(2)?.checked_add((i / 2) as i32)?;
                    self.overview_get(overview_key(
                        d,
                        layer,
                        LodKey {
                            x,
                            z,
                            level: node.level - 1,
                        },
                    ))
                });
                Overview::reduce(&children)
            };
            if layer == Layer::Surface
                && let Some(lod) = self.coarse_at(d, node)
            {
                for i in 0..256 {
                    image.fill(i, &lod.pixels[i * 4..i * 4 + 4]);
                }
            }
            let previous = self.overview_get(key);
            if previous.as_deref() == Some(&image)
                || previous.is_none()
                    && image.coverage.iter().all(|v| *v == 0)
                    && self
                        .backing
                        .as_ref()
                        .is_none_or(|b| b.borrow().store.version(key).is_none())
            {
                return Ok(());
            }
            if !self.read_only
                && let Some(backing) = &self.backing
            {
                backing.borrow_mut().store.put(key, image.encode())?;
            }
            if self.overviews.stale {
                self.overviews.fresh.insert(key);
            }
            if let Some((_, d, l, region, level, future, fine)) = self.overviews.goal
                && d == record_dimension(key).unwrap_or(Dimension::Player)
                && l == overview_layer(key)
                && (key[5] >= i32::from(level) && intersects(region, node.bounds())
                    || key[5] >= i32::from(fine) && intersects(future, node.bounds()))
                && self.overviews.wanted.len() < 32768
            {
                self.overviews.wanted.insert(key);
            }
            let capacity = (self.limit / 2).clamp(32 * 1024 * 1024, 64 * 1024 * 1024) / 1680;
            self.overviews.bad.remove(&key);
            self.overviews.remember(key, Arc::new(image), capacity);
            self.clock += 1;
            self.overviews.modified = true;
            if node.level < 22 {
                let parent = overview_key(
                    d,
                    layer,
                    LodKey {
                        x: node.x.div_euclid(2),
                        z: node.z.div_euclid(2),
                        level: node.level + 1,
                    },
                );
                self.overviews.dirty.insert((node.level + 1, parent));
            }
            Ok(())
        }
        fn overview_level(view: &View) -> u8 {
            let scale = (view.span[0] / view.width.max(1) as f64)
                .max(view.span[1] / view.height.max(1) as f64);
            let mut level = scale.max(1.).log2().floor().clamp(0., 22.) as u8;
            let region = view.atlas_region();
            while level < 22 {
                let size = (16_i64 << level) as f64;
                if (region[2] as f64 / size + 2.) * (region[3] as f64 / size + 2.) <= 32768. {
                    break;
                }
                level += 1;
            }
            level
        }
        fn query_visual(
            &self,
            view: &View,
            level: u8,
            region: [i64; 4],
            kind: i32,
            limit: usize,
        ) -> Vec<RecordKey> {
            let prefix = record_key(view.dimension, view.layer, 0, 0);
            self.backing.as_ref().map_or_else(Vec::new, |b| {
                b.borrow()
                    .store
                    .query_level(prefix[0], kind, prefix[2], level, region, limit)
            })
        }
        fn seed_presentation(&mut self, view: &View, target: &View) {
            let level = Self::overview_level(view);
            let target_level = Self::overview_level(target);
            let region = view.atlas_region();
            let future = target.atlas_region();
            let goal = (
                view.session,
                view.dimension,
                view.layer,
                region,
                level,
                future,
                target_level,
            );
            if self.overviews.goal == Some(goal) {
                return;
            }
            if self.overviews.goal.is_none_or(|old| {
                (old.0, old.1, old.2) != (view.session, view.dimension, view.layer)
            }) {
                self.overviews.displayed = false;
            }
            self.overviews.goal = Some(goal);
            self.overviews.wanted.clear();
            self.overviews.rebuild_order.clear();
            self.clock += 1;
            for (index, (v, l, r)) in [(view, level, region), (target, target_level, future)]
                .into_iter()
                .enumerate()
            {
                if index == 1 && region == future && level == target_level {
                    continue;
                }
                let prefix = record_key(v.dimension, v.layer, 0, 0);
                for level in (l..=22).rev() {
                    if !self.read_only || !self.overviews.stale {
                        self.overviews.wanted.extend(self.query_visual(
                            v,
                            level,
                            r,
                            prefix[1] + 3,
                            32768,
                        ));
                    }
                }
                let mut sources = self.backing.as_ref().map_or_else(Vec::new, |b| {
                    b.borrow()
                        .store
                        .query(prefix[0], prefix[1], prefix[2], r, 1024)
                });
                sources.extend(
                    self.tiles
                        .keys()
                        .filter(|k| {
                            k.0 == v.dimension
                                && k.1 == v.layer
                                && intersects(r, [i64::from(k.2) * 16, i64::from(k.3) * 16, 16, 16])
                        })
                        .map(|k| record_key(k.0, k.1, k.2, k.3)),
                );
                sources.sort_unstable();
                sources.dedup();
                sources.sort_unstable_by_key(|k| {
                    let dx = i64::from(k[3]) * 16 - v.center[0] as i64;
                    let dz = i64::from(k[4]) * 16 - v.center[1] as i64;
                    i128::from(dx).pow(2) + i128::from(dz).pow(2)
                });
                for source in sources.into_iter().take(1024) {
                    let key = self.seed_visual_source(source);
                    if self.overviews.dirty.contains(&key) {
                        self.overviews.rebuild_order.push_back(key.1);
                    }
                }
                if v.layer == Layer::Surface {
                    for level in (0..=22).rev() {
                        let keys = self.query_visual(v, level, r, 2, 256);
                        for source in keys {
                            self.overviews.wanted.insert(source);
                            self.seed_visual_source(source);
                        }
                    }
                    let sources: Vec<_> = self
                        .lod_tiles
                        .keys()
                        .filter(|(d, k)| *d == v.dimension && intersects(r, k.bounds()))
                        .map(|(d, k)| lod_record(*d, *k))
                        .collect();
                    for source in sources {
                        self.seed_visual_source(source);
                    }
                }
            }
        }
        fn bound_visual_wanted(&mut self, view: &View, target: &View) {
            let Some((_, _, _, _, level, future, fine)) = self.overviews.goal else {
                return;
            };
            let visible = view.visible_region();
            let near = view.atlas_region();
            let mut wanted: Vec<_> = self.overviews.wanted.iter().copied().collect();
            wanted.sort_unstable_by_key(|key| {
                let bounds = footprint(*key).bounds();
                let onscreen = intersects(visible, bounds);
                let priority = if onscreen && key[5] >= i32::from(level) + 3 {
                    // A few coarse records can provide immediate coverage.
                    0
                } else if onscreen && (key[5] == i32::from(level) || key[1] == 2) {
                    1
                } else if onscreen {
                    2
                } else if key[5] == i32::from(fine) && intersects(future, bounds) {
                    3
                } else if intersects(near, bounds) {
                    4
                } else {
                    5
                };
                let center = if priority < 3 {
                    view.center
                } else {
                    target.center
                };
                (
                    priority,
                    (i128::from(bounds[0] + bounds[2] / 2) - center[0] as i128).pow(2)
                        + (i128::from(bounds[1] + bounds[3] / 2) - center[1] as i128).pow(2),
                    std::cmp::Reverse(key[5]),
                )
            });
            wanted.truncate(32768);
            self.overviews.wanted = wanted.iter().copied().collect();
            self.overviews.load_order = wanted
                .into_iter()
                .filter(|key| {
                    !self.overviews.tiles.contains_key(key) && !self.overviews.bad.contains(key)
                })
                .collect();
            self.overviews.order = self
                .overviews
                .tiles
                .iter()
                .filter(|(k, _)| !self.overviews.wanted.contains(*k))
                .map(|(k, (stamp, _))| (*stamp, *k))
                .collect();
        }
        fn seed_visual_source(&mut self, source: RecordKey) -> (u8, RecordKey) {
            let mut key = source;
            key[1] = if source[1] == 1 { 4 } else { 3 };
            key[5] = if source[1] == 2 { source[5] } else { 0 };
            let resident = self.overviews.tiles.contains_key(&key);
            let stored = self
                .backing
                .as_ref()
                .is_some_and(|backing| backing.borrow().store.version(key).is_some());
            let known = if self.overviews.stale {
                if self.read_only {
                    resident
                } else {
                    (resident || stored) && self.overviews.fresh.contains(&key)
                }
            } else {
                resident || stored
            };
            let tag = (key[5] as u8, key);
            if !known || self.overviews.dirty.contains(&tag) {
                self.overviews.mark(source, true);
            }
            tag
        }
        fn load_saved_visuals(&mut self, start: Instant, budget: Duration, limit: usize) {
            for _ in 0..limit {
                if start.elapsed() >= budget {
                    break;
                }
                let Some(key) = self.overviews.load_order.pop_front() else {
                    break;
                };
                if !self.overviews.tiles.contains_key(&key) {
                    self.overview_get(key);
                }
            }
        }

        fn presentation_step(
            &mut self,
            view: &View,
            target: &View,
            shading: Option<&Shading>,
        ) -> io::Result<()> {
            let _timing = super::profiling::measure("visual-stream");
            let old_goal = self.overviews.goal;
            self.seed_presentation(view, target);
            if old_goal != self.overviews.goal {
                self.bound_visual_wanted(view, target);
            }
            let started = Instant::now();
            self.load_saved_visuals(started, Duration::from_millis(5), 128);
            if !self.overviews.done {
                let sources = self.backing.as_ref().map_or_else(Vec::new, |b| {
                    b.borrow()
                        .store
                        .source_keys_after(self.overviews.cursor, 32)
                });
                if sources.is_empty() {
                    self.overviews.done = true;
                } else {
                    self.overviews.cursor = sources.last().copied();
                    for source in sources {
                        self.overviews.mark(source, false);
                    }
                }
                self.overviews.modified = true;
            }
            let rebuilding = Instant::now();
            for _ in 0..256 {
                if rebuilding.elapsed() >= Duration::from_millis(6) {
                    break;
                }
                let next = self
                    .overviews
                    .rebuild_order
                    .pop_front()
                    .map(|key| {
                        let tag = (key[5] as u8, key);
                        self.overviews.urgent.remove(&tag);
                        tag
                    })
                    .or_else(|| self.overviews.urgent.pop_first())
                    .or_else(|| self.overviews.dirty.first().copied());
                let Some((level, key)) = next else {
                    break;
                };
                if !self.overviews.dirty.remove(&(level, key)) {
                    continue;
                }
                if let Err(error) = self.rebuild_overview(key, shading) {
                    self.overviews.dirty.insert((level, key));
                    return Err(error);
                }
                self.overviews.modified = true;
            }
            if !self.read_only
                && self.overviews.done
                && self.overviews.dirty.is_empty()
                && self.overviews.stale
            {
                self.overviews.stale = false;
                self.overviews.fresh.clear();
                self.overviews.modified = true;
            }
            self.load_saved_visuals(started, Duration::from_millis(12), 128);
            self.trim()?;
            Ok(())
        }
        fn visual_work(&self) -> bool {
            !self.overviews.done
                || !self.overviews.dirty.is_empty()
                || !self.overviews.load_order.is_empty()
        }
        fn prepared_atlas(
            &self,
            view: &View,
            target: &View,
            cached: &mut Option<AtlasCache>,
        ) -> Arc<TerrainAtlas> {
            let key = (view.session, view.dimension, view.layer, self.clock);
            if let Some(atlas) = cached
                .as_ref()
                .filter(|atlas| atlas.key == key && atlas.terrain.covers(view))
            {
                return atlas.terrain.clone();
            }
            let previous = cached.as_ref().map(|atlas| atlas.terrain.as_ref());
            let terrain = Arc::new(self.visual_atlas(view, target, previous));
            *cached = Some(AtlasCache {
                key,
                terrain: terrain.clone(),
            });
            terrain
        }

        fn visual_atlas(
            &self,
            view: &View,
            target: &View,
            previous: Option<&TerrainAtlas>,
        ) -> TerrainAtlas {
            let _timing = super::profiling::measure("visual-atlas");
            let previous =
                previous.filter(|p| p.scope == (view.session, view.dimension, view.layer));
            let region = view.atlas_region();
            let future = target.atlas_region();
            let level = Self::overview_level(view);
            let fine = Self::overview_level(target);
            let mut candidates: Vec<_> = self
                .overviews
                .tiles
                .iter()
                .filter(|(key, (_, tile))| {
                    record_dimension(**key) == Some(view.dimension)
                        && overview_layer(**key) == view.layer
                        && tile.coverage.iter().any(|c| *c != 0)
                        && ((key[5] >= i32::from(level)
                            && intersects(region, footprint(**key).bounds()))
                            || key[5] >= i32::from(fine)
                                && intersects(future, footprint(**key).bounds())
                            || key[1] == 2
                                && (intersects(region, footprint(**key).bounds())
                                    || intersects(future, footprint(**key).bounds())))
                })
                .map(|(key, (_, tile))| (*key, tile.clone()))
                .collect();
            candidates.sort_unstable_by_key(|(k, _)| {
                (
                    if k[5] >= i32::from(level.max(fine)) + 4 {
                        0
                    } else if k[5] == i32::from(fine) && intersects(future, footprint(*k).bounds())
                    {
                        1
                    } else if k[5] == i32::from(level) {
                        2
                    } else {
                        3
                    },
                    std::cmp::Reverse(k[5]),
                    k[1],
                    std::cmp::Reverse(k[2]),
                )
            });
            let mut nodes: BTreeMap<LodKey, Option<Arc<Overview>>> = BTreeMap::new();
            for (record, tile) in candidates.into_iter().take(32512) {
                let key = footprint(record);
                if record[1] == 2
                    && key.level < level.min(fine)
                    && (key.level + 1..=level.min(fine)).any(|l| {
                        let shift = l - key.level;
                        let parent = overview_key(
                            view.dimension,
                            view.layer,
                            LodKey {
                                x: key.x.div_euclid(1 << shift),
                                z: key.z.div_euclid(1 << shift),
                                level: l,
                            },
                        );
                        self.overviews.tiles.contains_key(&parent)
                            && !self.overviews.dirty.contains(&(l, parent))
                    })
                {
                    continue;
                }
                if record[1] == 2 {
                    nodes.entry(key).or_insert(Some(tile));
                } else {
                    nodes.insert(key, Some(tile));
                }
            }
            if let Some(old) = previous {
                let mut keep: Vec<_> = old
                    .coarse
                    .iter()
                    .map(|d| d.key)
                    .chain(old.chunks.iter().map(|c| LodKey {
                        x: c[0],
                        z: c[1],
                        level: 0,
                    }))
                    .filter(|k| old.sources.contains_key(&VisualKey::Lod(*k)))
                    .filter(|k| intersects(region, k.bounds()) || intersects(future, k.bounds()))
                    .collect();
                keep.sort_unstable_by_key(|k| std::cmp::Reverse(k.level));
                for key in keep {
                    if nodes.len() >= 32768 {
                        break;
                    }
                    nodes.entry(key).or_insert(None);
                }
            }
            let selected: BTreeSet<_> = nodes.keys().copied().collect();
            let mut ancestors = BTreeSet::new();
            for key in &selected {
                let mut key = *key;
                while key.level < 22 {
                    key = LodKey {
                        x: key.x.div_euclid(2),
                        z: key.z.div_euclid(2),
                        level: key.level + 1,
                    };
                    ancestors.insert(key);
                }
            }
            let compact = previous.is_some_and(|p| nodes.len() * 4 < p.locations.len());
            let mut locations: BTreeMap<_, _> = previous
                .filter(|_| !compact)
                .map(|p| {
                    p.locations
                        .iter()
                        .filter(|(k, _)| matches!(k,VisualKey::Lod(key) if selected.contains(key)))
                        .map(|(k, v)| (*k, *v))
                        .collect()
                })
                .unwrap_or_default();
            let mut used: BTreeSet<_> = locations.values().copied().collect();
            let mut next = 0;
            for key in &selected {
                if locations.contains_key(&VisualKey::Lod(*key)) {
                    continue;
                }
                while used.contains(&next) {
                    next += 1;
                }
                locations.insert(VisualKey::Lod(*key), next);
                used.insert(next);
            }
            let height = used.last().map_or(16, |s| (s + 1).div_ceil(64) * 16);
            let mut pixels = vec![0; 1024 * height * 4];
            let mut sources = BTreeMap::new();
            let mut coarse = Vec::new();
            let mut chunks = Vec::new();
            let mut slots = Vec::new();
            let mut prepared_tiles = 0;
            for (key, image) in nodes {
                let slot = locations[&VisualKey::Lod(key)];
                let mut parents = Vec::new();
                let mut dependencies = Vec::new();
                if image
                    .as_ref()
                    .is_some_and(|i| i.coverage.iter().any(|v| *v != u16::MAX))
                    && view.layer == Layer::Surface
                {
                    for level in key.level + 1..=22 {
                        let shift = level - key.level;
                        let parent = LodKey {
                            x: key.x.div_euclid(1 << shift),
                            z: key.z.div_euclid(1 << shift),
                            level,
                        };
                        let mut legacy = lod_record(view.dimension, parent);
                        legacy[2] = 0;
                        let tile = self
                            .overviews
                            .tiles
                            .get(&lod_record(view.dimension, parent))
                            .or_else(|| self.overviews.tiles.get(&legacy))
                            .or_else(|| {
                                self.overviews.tiles.get(&overview_key(
                                    view.dimension,
                                    view.layer,
                                    parent,
                                ))
                            })
                            .map(|(_, t)| t.clone());
                        if let Some(tile) = tile {
                            dependencies.push((
                                parent,
                                VisualSource::Image(Arc::downgrade(&tile), Vec::new()),
                            ));
                            parents.push((parent, tile));
                        }
                    }
                }
                let signature = if image.is_none()
                    && previous.is_some_and(|p| p.sources.contains_key(&VisualKey::Lod(key)))
                {
                    previous.unwrap().sources[&VisualKey::Lod(key)].clone()
                } else {
                    [
                        if let Some(image) = &image {
                            VisualSource::Image(Arc::downgrade(image), dependencies)
                        } else {
                            self.visual_source(lod_record(view.dimension, key))
                        },
                        VisualSource::Stored(None),
                        VisualSource::Stored(None),
                    ]
                };
                let reusable = previous.and_then(|p| {
                    p.sources
                        .get(&VisualKey::Lod(key))
                        .filter(|old| old.iter().zip(&signature).all(|(a, b)| a.same(b)))
                        .map(|_| p)
                });
                let tile_pixels = if reusable.is_none() {
                    prepared_tiles += 1;
                    if let Some(image) = image {
                        let mut image = (*image).clone();
                        let bounds = key.bounds();
                        let stride = bounds[2] / 16;
                        for (parent, tile) in &parents {
                            for i in 0..256 {
                                if image.coverage[i] == u16::MAX {
                                    continue;
                                }
                                let at = [
                                    (bounds[0] + (i % 16) as i64 * stride + stride / 2) as i32,
                                    (bounds[1] + (i / 16) as i64 * stride + stride / 2) as i32,
                                ];
                                if let Some(cell) = parent.cell(at) {
                                    image.fill_partial(
                                        i,
                                        &tile.pixels[cell * 4..cell * 4 + 4],
                                        tile.coverage[cell],
                                    );
                                }
                            }
                        }
                        Some(image.pixels)
                    } else {
                        Some(vec![0; 1024])
                    }
                } else {
                    None
                };
                for row in 0..16 {
                    let at = ((slot / 64 * 16 + row) * 1024 + slot % 64 * 16) * 4;
                    if let Some(tile) = &tile_pixels {
                        pixels[at..at + 64].copy_from_slice(&tile[row * 64..row * 64 + 64]);
                    } else {
                        let old = reusable.unwrap();
                        let s = old.locations[&VisualKey::Lod(key)];
                        let from = ((s / 64 * 16 + row) * 1024 + s % 64 * 16) * 4;
                        pixels[at..at + 64].copy_from_slice(&old.pixels[from..from + 64]);
                    }
                }
                sources.insert(VisualKey::Lod(key), signature);
                let mut fragments = Vec::new();
                let mut stack = vec![key];
                while let Some(part) = stack.pop() {
                    if part != key && selected.contains(&part) {
                        continue;
                    }
                    let bounds = part.bounds();
                    if ancestors.contains(&part) && part.level > 0 {
                        for z in 0..2 {
                            for x in 0..2 {
                                stack.push(LodKey {
                                    x: part.x * 2 + x,
                                    z: part.z * 2 + z,
                                    level: part.level - 1,
                                });
                            }
                        }
                    } else {
                        fragments.push(bounds);
                    }
                }
                let detail = record_key(view.dimension, view.layer, key.x, key.z);
                let is_detail = key.level == 0
                    && (self
                        .tiles
                        .contains_key(&(view.dimension, view.layer, key.x, key.z))
                        || self.backing.as_ref().is_some_and(|b| {
                            let b = b.borrow();
                            b.store.version(detail).is_some() && !b.unreadable.contains(&detail)
                        }));
                if is_detail {
                    chunks.push([key.x, key.z]);
                    slots.push(slot);
                } else {
                    coarse.push(LodDraw {
                        key,
                        uv_origin: [slot % 64 * 16, slot / 64 * 16],
                        fragments,
                    });
                }
            }
            coarse.sort_by_key(|d| (std::cmp::Reverse(d.key.level), d.uv_origin[1] / 64));
            let pages = pixels
                .chunks(1024 * 64 * 4)
                .enumerate()
                .map(|(i, bytes)| {
                    previous
                        .and_then(|p| p.pages.get(i))
                        .filter(|p| p.pixels == bytes)
                        .cloned()
                        .unwrap_or_else(|| {
                            Arc::new(AtlasPage {
                                pixels: bytes.to_vec(),
                                width: 1024,
                                height: bytes.len() / 4096,
                            })
                        })
                })
                .collect();
            let mut draw_order: Vec<_> = (0..chunks.len()).collect();
            draw_order.sort_unstable_by_key(|i| slots[*i] / 256);
            TerrainAtlas {
                pixels,
                height,
                chunks,
                coarse,
                region,
                width: 1024,
                slots,
                sources,
                locations,
                scope: (view.session, view.dimension, view.layer),
                prepared_tiles,
                pages,
                draw_order,
            }
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
            let tile = self
                .lod_tiles
                .get(&(d, key))
                .map(|(_, t)| t.clone())
                .or_else(|| {
                    let mut backing = self.backing.as_ref()?.borrow_mut();
                    let mut record = lod_record(d, key);
                    backing.get(record).or_else(|| {
                        record[2] = 0;
                        backing.get(record)
                    })
                });
            let mut record = lod_record(d, key);
            record[2] = 0;
            if let Some(tile) = tile
                .as_ref()
                .filter(|t| t.pixels.as_chunks::<4>().0.iter().any(|p| p[3] == 0))
                && let Some(old) = self
                    .backing
                    .as_ref()
                    .and_then(|b| b.borrow_mut().get(record))
            {
                let mut combined = (**tile).clone();
                fill_lod_unknown(&mut combined, &old);
                return Some(Arc::new(combined));
            }
            tile
        }
        #[cfg(test)]
        fn prepare(&mut self, view: &View) -> io::Result<()> {
            let _timing = super::profiling::measure("residency");
            let Some(backing) = &self.backing else {
                return Ok(());
            };
            let region = view.atlas_region();
            // Bound even the temporary list when a zoomed-out view covers an
            // enormous history; raster reads query the remaining keys on demand.
            let candidates = (self.limit / 4096).max(1);
            let prefix = record_key(view.dimension, view.layer, 0, 0);
            let mut keys = backing.borrow().store.query(
                prefix[0],
                prefix[1],
                prefix[2],
                region,
                candidates + 1,
            );
            if view.layer == Layer::Surface && keys.len() <= candidates {
                keys.extend(backing.borrow().store.query(
                    prefix[0],
                    2,
                    0,
                    region,
                    candidates + 1 - keys.len(),
                ));
            }
            // Prioritize the camera within bounded admitted candidates.
            keys.sort_by_key(|key| {
                let size = if key[1] == 2 {
                    16_i64 << key[5].clamp(0, 22)
                } else {
                    16
                };
                let dx = key[3] as f64 * size as f64 + size as f64 / 2.0 - view.center[0];
                let dz = key[4] as f64 * size as f64 + size as f64 / 2.0 - view.center[1];
                (dx * dx + dz * dz) as u64
            });
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
                b.read_order = b
                    .reads
                    .iter()
                    .map(|(key, (stamp, _))| (*stamp, *key))
                    .collect();
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
                || !tile.pixels.as_chunks::<4>().0.iter().any(|p| p[3] != 0)
            {
                return false;
            }
            *self.lod_levels.entry(dimension).or_default() |= 1 << key.level;
            let recolor = !self.lod_current_colors.contains(&(dimension, key))
                && self.backing.as_ref().is_some_and(|b| {
                    b.borrow()
                        .store
                        .version(lod_record(dimension, key))
                        .is_none()
                });
            self.lod_current_colors.insert((dimension, key));
            if !self.lod_tiles.contains_key(&(dimension, key))
                && let Some(old) = self.coarse_at(dimension, key)
            {
                self.lod_tiles.insert((dimension, key), (self.clock, old));
            }
            if let Some((stamp, old)) = self.lod_tiles.get_mut(&(dimension, key)) {
                let old = Arc::make_mut(old);
                // Old-palette coverage stays in its original record. Persist
                // only newly observed colours, so a later partial snapshot can
                // still replace legacy cells instead of treating them as final.
                if recolor {
                    old.pixels.fill(0);
                }
                let mut changed = recolor;
                for i in 0..256 {
                    if (recolor || old.pixels[i * 4 + 3] == 0) && tile.pixels[i * 4 + 3] != 0 {
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
                        for (i, pixel) in tile.pixels.as_chunks::<4>().0.iter().enumerate() {
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
        #[cfg(test)]
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
        #[cfg(test)]
        fn gpu_atlas_fits(&self, view: &View) -> bool {
            !self.visual_keys(view).2
        }
        #[cfg(test)]
        fn visual_keys(&self, view: &View) -> (Vec<[i32; 2]>, Vec<LodKey>, bool) {
            let region = view.atlas_region();
            let capacity = if self.limit == 0 {
                32_768
            } else {
                (self.limit / 4096).clamp(4096, 32_768)
            };
            let mut fine: BTreeSet<_> = self
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
            let mut coarse: BTreeSet<_> = if view.layer == Layer::Surface {
                self.lod_tiles
                    .keys()
                    .filter(|(d, key)| *d == view.dimension && intersects(region, key.bounds()))
                    .map(|(_, key)| *key)
                    .collect()
            } else {
                BTreeSet::new()
            };
            if let Some(backing) = &self.backing {
                let backing = backing.borrow();
                let key = record_key(view.dimension, view.layer, 0, 0);
                fine.extend(
                    backing
                        .store
                        .query(key[0], key[1], key[2], region, capacity + 1)
                        .into_iter()
                        .map(|k| [k[3], k[4]]),
                );
                if view.layer == Layer::Surface {
                    coarse.extend(
                        backing
                            .store
                            .query(key[0], 2, 0, region, capacity + 1)
                            .into_iter()
                            .map(|k| LodKey {
                                x: k[3],
                                z: k[4],
                                level: k[5] as u8,
                            }),
                    );
                }
                fine.retain(|c| {
                    !backing.unreadable.contains(&record_key(
                        view.dimension,
                        view.layer,
                        c[0],
                        c[1],
                    ))
                });
                coarse.retain(|key| {
                    !backing
                        .unreadable
                        .contains(&lod_record(view.dimension, *key))
                });
            }
            let oversized = fine.len() + coarse.len() > capacity;
            (
                fine.into_iter().take(capacity + 1).collect(),
                coarse.into_iter().take(capacity + 1).collect(),
                oversized,
            )
        }
        fn shaded_chunk(&self, dimension: Dimension, layer: Layer, chunk: [i32; 2]) -> Vec<u8> {
            let Some(tile) = self.tile_at((dimension, layer, chunk[0], chunk[1])) else {
                return vec![0; 1024];
            };
            // Neighbour handles are resolved once per tile, never once per block.
            let north = self.tile_at((dimension, layer, chunk[0], chunk[1] - 1));
            let west = self.tile_at((dimension, layer, chunk[0] - 1, chunk[1]));
            Self::shade_pixels(&tile, north.as_deref(), west.as_deref())
        }
        fn shade_pixels(tile: &Tile, north: Option<&Tile>, west: Option<&Tile>) -> Vec<u8> {
            let mut pixels = tile.pixels.clone();
            for z in 0..16 {
                for x in 0..16 {
                    let i = z * 16 + x;
                    let height = tile.heights[i];
                    let n = if z > 0 {
                        tile.heights[i - 16]
                    } else {
                        north.as_ref().map_or(height, |t| t.heights[240 + x])
                    };
                    let w = if x > 0 {
                        tile.heights[i - 1]
                    } else {
                        west.as_ref().map_or(height, |t| t.heights[z * 16 + 15])
                    };
                    let relief = f32::from(n.saturating_sub(height).clamp(-64, 64) as i8) * -0.085
                        + f32::from(w.saturating_sub(height).clamp(-64, 64) as i8) * -0.05;
                    let shade = if tile.walls[i] {
                        0.55
                    } else {
                        (1.09 + relief).clamp(0.65, 1.38)
                    };
                    for channel in 0..3 {
                        pixels[i * 4 + channel] =
                            (f32::from(pixels[i * 4 + channel]) * shade).round() as u8;
                    }
                }
            }
            pixels
        }
        fn visual_source(&self, key: RecordKey) -> VisualSource {
            if let Some(backing) = &self.backing {
                return VisualSource::Stored(backing.borrow().store.version(key));
            }
            let tile = record_dimension(key).and_then(|d| {
                if let Some(layer) = record_layer(key) {
                    self.tiles.get(&(d, layer, key[3], key[4])).map(|(_, t)| t)
                } else {
                    self.lod_tiles
                        .get(&(
                            d,
                            LodKey {
                                x: key[3],
                                z: key[4],
                                level: key[5] as u8,
                            },
                        ))
                        .map(|(_, t)| t)
                }
            });
            VisualSource::Memory(tile.map(Arc::downgrade).unwrap_or_default())
        }
        #[cfg(test)]
        fn prepare_terrain_atlas(
            &self,
            view: &View,
            previous: Option<&TerrainAtlas>,
            shading: Option<&Shading>,
        ) -> TerrainAtlas {
            let _timing = super::profiling::measure("atlas");
            let previous =
                previous.filter(|p| p.scope == (view.session, view.dimension, view.layer));
            let region = view.atlas_region();
            let (mut chunks, mut coarse, _) = self.visual_keys(view);
            let detailed: BTreeSet<_> = chunks.iter().copied().collect();
            coarse.sort_by_key(|key| std::cmp::Reverse(key.level));
            let keys: BTreeSet<_> = chunks
                .iter()
                .copied()
                .map(VisualKey::Detail)
                .chain(coarse.iter().copied().map(VisualKey::Lod))
                .collect();
            let mut locations: BTreeMap<_, _> = previous
                .map(|p| {
                    p.locations
                        .iter()
                        .filter(|(k, _)| keys.contains(k))
                        .map(|(k, v)| (*k, *v))
                        .collect()
                })
                .unwrap_or_default();
            let mut used: BTreeSet<_> = locations.values().copied().collect();
            let mut next = 0;
            for key in chunks
                .iter()
                .copied()
                .map(VisualKey::Detail)
                .chain(coarse.iter().copied().map(VisualKey::Lod))
            {
                if locations.contains_key(&key) {
                    continue;
                }
                while used.contains(&next) {
                    next += 1;
                }
                locations.insert(key, next);
                used.insert(next);
            }
            let mut coarse: Vec<LodDraw> = coarse
                .into_iter()
                .map(|key| {
                    let slot = locations[&VisualKey::Lod(key)];
                    LodDraw {
                        key,
                        uv_origin: [slot % 64 * 16, slot / 64 * 16],
                        fragments: lod_fragments(key, &detailed),
                    }
                })
                .collect();
            let height = (used.last().map_or(1, |s| (s + 1).div_ceil(64)) * 16)
                .max(previous.map_or(0, |p| p.height));
            let mut pixels = vec![0; 1024 * height * 4];
            let mut sources = BTreeMap::new();
            let mut prepared_tiles = 0;
            let mut pending = Vec::new();
            for (&key, &slot) in &locations {
                let signature = match key {
                    VisualKey::Detail([x, z]) => [
                        self.visual_source(record_key(view.dimension, view.layer, x, z)),
                        self.visual_source(record_key(
                            view.dimension,
                            view.layer,
                            x,
                            z.saturating_sub(1),
                        )),
                        self.visual_source(record_key(
                            view.dimension,
                            view.layer,
                            x.saturating_sub(1),
                            z,
                        )),
                    ],
                    VisualKey::Lod(k) => [
                        self.visual_source(lod_record(view.dimension, k)),
                        VisualSource::Stored(None),
                        VisualSource::Stored(None),
                    ],
                };
                let reusable = previous.filter(|p| {
                    p.locations.get(&key) == Some(&slot)
                        && p.sources
                            .get(&key)
                            .is_some_and(|old| old.iter().zip(&signature).all(|(a, b)| a.same(b)))
                });
                let tile_pixels = if reusable.is_none() {
                    prepared_tiles += 1;
                    if let (Some(shading), VisualKey::Detail([x, z])) = (shading, key)
                        && let Some(tile) = self.tile_at((view.dimension, view.layer, x, z))
                        && let Some(result) = shading.submit(
                            tile,
                            self.tile_at((view.dimension, view.layer, x, z.saturating_sub(1))),
                            self.tile_at((view.dimension, view.layer, x.saturating_sub(1), z)),
                        )
                    {
                        pending.push((slot, [x, z], result));
                        sources.insert(key, signature);
                        continue;
                    }
                    Some(match key {
                        VisualKey::Detail(chunk) => {
                            self.shaded_chunk(view.dimension, view.layer, chunk)
                        }
                        VisualKey::Lod(key) => self
                            .coarse_at(view.dimension, key)
                            .map_or_else(|| vec![0; 1024], |t| t.pixels.clone()),
                    })
                } else {
                    None
                };
                for row in 0..16 {
                    let target = ((slot / 64 * 16 + row) * 1024 + slot % 64 * 16) * 4;
                    let source = if let Some(tile) = &tile_pixels {
                        &tile[row * 64..row * 64 + 64]
                    } else {
                        &reusable.unwrap().pixels[target..target + 64]
                    };
                    pixels[target..target + 64].copy_from_slice(source);
                }
                sources.insert(key, signature);
            }
            for (slot, chunk, result) in pending {
                let tile = result
                    .recv()
                    .unwrap_or_else(|_| self.shaded_chunk(view.dimension, view.layer, chunk));
                for row in 0..16 {
                    let at = ((slot / 64 * 16 + row) * 1024 + slot % 64 * 16) * 4;
                    pixels[at..at + 64].copy_from_slice(&tile[row * 64..row * 64 + 64]);
                }
            }
            super::profiling::record("shaded-tiles", Duration::ZERO, prepared_tiles as u64, 0);
            if let Some(backing) = &self.backing {
                let backing = backing.borrow();
                chunks.retain(|c| {
                    !backing.unreadable.contains(&record_key(
                        view.dimension,
                        view.layer,
                        c[0],
                        c[1],
                    ))
                });
                coarse.retain(|c| {
                    !backing
                        .unreadable
                        .contains(&lod_record(view.dimension, c.key))
                });
                let detailed = chunks.iter().copied().collect();
                for draw in &mut coarse {
                    draw.fragments = lod_fragments(draw.key, &detailed);
                }
            }
            let slots: Vec<_> = chunks
                .iter()
                .map(|key| locations[&VisualKey::Detail(*key)])
                .collect();
            let mut draw_order: Vec<_> = (0..chunks.len()).collect();
            draw_order.sort_unstable_by_key(|i| slots[*i] / 256);
            coarse.sort_by_key(|draw| (std::cmp::Reverse(draw.key.level), draw.uv_origin[1] / 64));
            let pages = if height > 1024 {
                pixels
                    .chunks(1024 * 64 * 4)
                    .enumerate()
                    .map(|(i, bytes)| {
                        if let Some(page) = previous
                            .and_then(|p| p.pages.get(i))
                            .filter(|p| p.pixels == bytes)
                        {
                            page.clone()
                        } else {
                            Arc::new(AtlasPage {
                                pixels: bytes.to_vec(),
                                width: 1024,
                                height: bytes.len() / (1024 * 4),
                            })
                        }
                    })
                    .collect()
            } else {
                Vec::new()
            };
            TerrainAtlas {
                pixels,
                height,
                chunks,
                coarse,
                region,
                width: 1024,
                slots,
                sources,
                locations,
                scope: (view.session, view.dimension, view.layer),
                prepared_tiles,
                pages,
                draw_order,
            }
        }
        pub fn render(&self, mut view: View, serial: u64) -> Frame {
            let _timing = super::profiling::measure("raster");
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
            let levels = if view.layer == Layer::Surface {
                self.lod_levels.get(&view.dimension).copied().unwrap_or(0)
            } else {
                0
            };
            let mut lod: [Option<(LodKey, Option<Arc<Tile>>)>; 23] = std::array::from_fn(|_| None);
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
                        let mut remaining = levels;
                        while remaining != 0 {
                            let level = remaining.trailing_zeros() as usize;
                            remaining &= remaining - 1;
                            let size = 16_i64 << level;
                            let key = LodKey {
                                x: i64::from(bx).div_euclid(size) as i32,
                                z: i64::from(bz).div_euclid(size) as i32,
                                level: level as u8,
                            };
                            if lod[level].as_ref().is_none_or(|(old, _)| *old != key) {
                                lod[level] = Some((key, self.coarse_at(view.dimension, key)));
                            }
                            if let Some(tile) =
                                lod[level].as_ref().and_then(|(_, tile)| tile.as_ref())
                            {
                                let cell = key.cell([bx, bz]).unwrap();
                                let color = &tile.pixels[cell * 4..cell * 4 + 4];
                                if color[3] != 0 {
                                    pixels[i..i + 4].copy_from_slice(color);
                                    known += 1;
                                    break;
                                }
                            }
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
                        Some(current.as_ref())
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
                        north.as_ref().and_then(|(_, tile)| tile.as_deref())
                    };
                    let west_x = bx - 1;
                    let west_chunk = (west_x.div_euclid(16), bz.div_euclid(16));
                    let west_tile = if west_chunk == chunk {
                        Some(current.as_ref())
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
                        west.as_ref().and_then(|(_, tile)| tile.as_deref())
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
        Delete {
            selection: container::DeleteSelection,
            active_world: String,
            reply: mpsc::SyncSender<Result<(), String>>,
        },
        Rebuild {
            world: String,
            file: String,
            reply: mpsc::SyncSender<Result<(), String>>,
        },
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
    type MapProbe = (u64, Dimension, Layer, [i32; 2]);

    #[derive(Clone, PartialEq, Eq)]
    struct Binding {
        session: u64,
        world: String,
        dimension: Dimension,
        limit: usize,
    }
    type PositionSlot = Arc<Mutex<BTreeMap<u64, LastPosition>>>;
    pub struct Service {
        position: PositionSlot,
        binding: Arc<Mutex<Option<Binding>>>,
        upgrade: Arc<Mutex<Option<String>>>,
        status: Arc<Mutex<Option<String>>>,
        paused: Arc<std::sync::atomic::AtomicBool>,
        tx: Mutex<Option<mpsc::SyncSender<Job>>>,
        worker: Mutex<Option<JoinHandle<()>>>,
        latest: Arc<Mutex<Option<View>>>,
        sampler: Mutex<Option<JoinHandle<()>>>,
        lookahead: Arc<Mutex<Option<View>>>,
        probe: Arc<Mutex<Option<MapProbe>>>,
        wake: Mutex<Option<mpsc::SyncSender<Job>>>,
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
            let position: PositionSlot = Arc::new(Mutex::new(BTreeMap::new()));
            let latest_position = position.clone();
            let upgrade = Arc::new(Mutex::new(None));
            let upgrade_notice = upgrade.clone();
            let (tx, sampling) = mpsc::sync_channel::<Job>(8);
            let (prepared, rx) = mpsc::sync_channel(8);
            let wake = prepared.clone();
            let latest = Arc::new(Mutex::new(None::<View>));
            let lookahead = Arc::new(Mutex::new(None::<View>));
            let probe = Arc::new(Mutex::new(None::<MapProbe>));
            let requested_probe = probe.clone();
            let prediction = lookahead.clone();
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
            let sampling_session = session.clone();
            let sampling_paused = paused.clone();
            let sampling_status = status.clone();
            let sampling_failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let cpu_failed = sampling_failed.clone();
            let sampler = thread::Builder::new()
                .name("map-sampling".into())
                .spawn(move || {
                    while let Ok(job) = sampling.recv() {
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let _timing = super::profiling::measure("sampling");
                            match job {
                                Job::Chunk {
                                    session,
                                    dimension,
                                    layer,
                                    chunk,
                                    sample,
                                } if session == sampling_session.load(Ordering::Acquire)
                                    && !sampling_paused.load(Ordering::Acquire) =>
                                {
                                    let tile = sample();
                                    Some(Job::Chunk {
                                        session,
                                        dimension,
                                        layer,
                                        chunk,
                                        sample: Box::new(move || tile),
                                    })
                                }
                                Job::Lod {
                                    session,
                                    dimension,
                                    key,
                                    sample,
                                } if session == sampling_session.load(Ordering::Acquire)
                                    && !sampling_paused.load(Ordering::Acquire) =>
                                {
                                    let tile = sample();
                                    Some(Job::Lod {
                                        session,
                                        dimension,
                                        key,
                                        sample: Box::new(move || tile),
                                    })
                                }
                                Job::Wake => Some(Job::Wake),
                                _ => None,
                            }
                        }));
                        match result {
                            Ok(Some(job)) => {
                                if prepared.send(job).is_err() {
                                    break;
                                }
                            }
                            Ok(None) => {}
                            Err(_) => {
                                sampling_failed.store(true, Ordering::Release);
                                sampling_paused.store(true, Ordering::Release);
                                *sampling_status.lock().unwrap_or_else(|e| e.into_inner()) =
                                    Some("Map sampling failed; sampling paused".into());
                                break;
                            }
                        }
                    }
                })
                .map_err(|e| eprintln!("Map sampling worker failed: {e}"))
                .ok();
            if sampler.is_none() {
                cpu_failed.store(true, Ordering::Release);
            }
            let worker = thread::Builder::new().name("map-terrain".into()).spawn(move || {
                    let shading = Shading::new();
                    let mut cache = Cache::default();
                    let mut current = 0;
                    let mut configured = None::<Binding>;
                    let mut serial = 0;
                    let mut atlas = None::<AtlasCache>;
                    let mut last_commit = Instant::now();
                    let mut paused = false;
                    let mut lease_retry = None::<Instant>;
                    let mut blocked_format=None;
                    let mut last_probe=None;
                    let mut saved_position = None;
                    loop {
                        let busy=cache.visual_work()&&input.lock().unwrap_or_else(|e|e.into_inner()).is_some();
                        let received = rx.recv_timeout(Duration::from_millis(if busy {1} else {100}));
                        if let Some(backing) = &cache.backing {
                            let latest = latest_position.lock().unwrap_or_else(|e| e.into_inner()).get(&current).copied().map(|position| (current, position));
                            if let Some((session, position)) = latest
                                && session == current && latest != saved_position && !paused
                            {
                                match backing.borrow_mut().store.put(LAST_POSITION, position.encode()) {
                                    Ok(()) => saved_position = latest,
                                    Err(e) => {
                                        paused = true;
                                        *errors.lock().unwrap_or_else(|e|e.into_inner()) = Some(format!("Cache write failed; sampling paused: {e}"));
                                    }
                                }
                            }
                        }
                        let first = match received {
                            Ok(job) => Some(job),
                            Err(mpsc::RecvTimeoutError::Timeout) => None,
                            Err(mpsc::RecvTimeoutError::Disconnected) => {
                                if let Err(e) = cache.flush() {
                                    eprintln!("Map cache flush on exit failed: {e}");
                                    *errors.lock().unwrap_or_else(|e| e.into_inner()) = Some(e.to_string());
                                }
                                paused_state.store(paused || cpu_failed.load(Ordering::Acquire), Ordering::Release);
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
                            if !paused && let Err(e) = cache.flush() {
                                paused = true;
                                *errors.lock().unwrap_or_else(|e|e.into_inner()) =
                                    Some(format!("Cache write failed; sampling paused: {e}"));
                            }
                            // Drop the old lease even after a failed write. Keep
                            // its error visible through disconnect.
                            cache = Cache::default();
                            blocked_format = None;
                            lease_retry = None;
                            if let Some(binding) = &wanted {
                                paused = cpu_failed.load(Ordering::Acquire);
                                cache.limit = binding.limit;
                                if !paused && directory.is_some() && !binding.world.is_empty() {
                                    lease_retry = Some(Instant::now());
                                } else if !paused {
                                    *errors.lock().unwrap_or_else(|e|e.into_inner()) = if binding.world.is_empty() && directory.is_some() {
                                        Some("Unknown world identity; temporary RAM map only".into())
                                    } else {
                                        None
                                    };
                                }
                            }
                            latest_position.lock().unwrap_or_else(|e| e.into_inner()).retain(|session, _| *session >= next);
                            current = next;
                            configured = wanted.clone();
                            atlas = None;
                            last_probe = None;
                            changed = true;
                            last_commit = Instant::now();
                        }
                        if let (Some(at), Some(directory), Some(binding)) = (lease_retry, &directory, &wanted)
                            && binding.session == next && Instant::now() >= at && !paused
                        {
                            match Cache::open(directory, &binding.world, binding.limit) {
                                Ok(mut opened) => {
                                    lease_retry = None;
                                    if let Err(error) = opened.merge_transient(&cache) {
                                        paused = true;
                                        *errors.lock().unwrap_or_else(|e|e.into_inner()) =
                                            Some(format!("Cache write failed; sampling paused: {error}"));
                                    } else {
                                        cache = opened;
                                        atlas = None;
                                        changed = true;
                                        last_probe = None;
                                        last_commit = Instant::now();
                                        *errors.lock().unwrap_or_else(|e|e.into_inner()) = None;
                                    }
                                }
                                Err(container::LeaseError::Busy) => {
                                    lease_retry = Some(Instant::now() + Duration::from_millis(250));
                                    *errors.lock().unwrap_or_else(|e|e.into_inner()) =
                                        Some("Cache is used by another client; temporary RAM map only".into());
                                }
                                Err(container::LeaseError::Io(e)) => {
                                    lease_retry = None;
                                    if e.to_string().contains("unsupported container version") {
                                        blocked_format = Some(binding.world.clone());
                                    }
                                    paused = true;
                                    *errors.lock().unwrap_or_else(|e|e.into_inner()) =
                                        Some(format!("Cannot open cache; sampling paused: {e}"));
                                }
                            }
                        }
                        if let (Some(old), Some(new)) = (&configured.clone(), &wanted) {
                            if old.dimension != new.dimension && !paused {
                                if let Err(e) = cache.flush() {
                                    paused = true;
                                    *errors.lock().unwrap_or_else(|e| e.into_inner()) =
                                        Some(format!("Cache write failed; sampling paused: {e}"));
                                } else {
                                    if lease_retry.is_none() {
                                        cache.clear_payloads();
                                    }
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
                b.read_order.clear();
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
                            let job = match job {
                                Job::Delete { selection, active_world, reply } => {
                                    let result = (|| {
                                        let directory = directory.as_ref().ok_or("Persistent cache is unavailable")?;
                                        if !active_world.is_empty() && container::world_path(directory, &active_world)
                                            .file_name().is_some_and(|name| name == selection.file.as_str())
                                        {
                                            return Err("Cache is in use by another client".into());
                                        }
                                        container::delete_selected(directory, &selection).map_err(|e| e.to_string())
                                    })();
                                    let _ = reply.try_send(result);
                                    continue;
                                }
                                Job::Rebuild { world, file, reply } => {
                                    let is_active = configured.as_ref().is_some_and(|b| b.world == world);
                                    let correct_file = directory.as_ref().is_some_and(|d| container::world_path(d,&world).file_name().is_some_and(|n| n == file.as_str()));
                                    let result = if !correct_file {
                                        Err("Open this cache's world before rebuilding its legacy file".into())
                                    } else if is_active && paused {
                                        Err("Cache writer is paused; resolve the write error first".into())
                                    } else if is_active && lease_retry.is_some() {
                                        Err("Cache is in use by another client".into())
                                    } else if is_active {
                                        cache.schedule_visual_rebuild().map_err(|e|e.to_string())
                                    } else if let Some(directory) = &directory {
                                            match Store::open_existing(&container::world_path(directory,&world), &world).and_then(|store|Cache::from_store(store,16 * 1024 * 1024)) {
                                                Ok(mut other) => other.schedule_visual_rebuild().map_err(|e|e.to_string()),
                                                Err(container::LeaseError::Busy) => Err("Cache is in use by another client".into()),
                                                Err(container::LeaseError::Io(e)) => Err(e.to_string()),
                                            }
                                    } else { Err("Persistent cache is unavailable".into()) };
                                    if is_active && correct_file && result.is_err() && !paused {
                                        paused = true;
                                        *errors.lock().unwrap_or_else(|e|e.into_inner()) = result.as_ref().err().cloned();
                                    }
                                    let _ = reply.try_send(result);
                                    continue;
                                }
                                other => other,
                            };
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
                            super::profiling::report();
                        }
                        paused |= cpu_failed.load(Ordering::Acquire);
                        paused_state.store(paused, Ordering::Release);
                        *upgrade_notice.lock().unwrap_or_else(|e|e.into_inner())=configured.as_ref().filter(|_|cache.backing.as_ref().is_some_and(|b|b.borrow().store.upgrade_pending)).map(|b|b.world.clone()).or_else(||blocked_format.clone());
                        let Some(view) = input.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
                            continue;
                        };
                        if view.session != current || view.session != active.load(Ordering::Acquire) {
                            continue;
                        }
                        let point=*requested_probe.lock().unwrap_or_else(|e|e.into_inner());
                        if let Some((session,d,layer,at))=point.filter(|(s,d,l,_)|*s==current&&*d==view.dimension&&*l==view.layer) {
                            let chunk=[at[0].div_euclid(16),at[1].div_euclid(16)];
                            let tag=(session,d,layer,chunk);
                            if last_probe!=Some(tag) {
                                cache.tile_at((d,layer,chunk[0],chunk[1]));
                                if layer==Layer::Surface {
                                    for level in 0..=22 {
                                        let size=16_i64<<level;
                                        let k=LodKey{x:i64::from(at[0]).div_euclid(size) as i32,z:i64::from(at[1]).div_euclid(size) as i32,level};
                                        if let Some(tile)=cache.coarse_at(d,k) && k.cell(at).is_some_and(|i|tile.pixels[i*4+3]!=0){break;}
                                    }
                                }
                                let mut summary=cache.coverage();summary.session=current;
                                let mut points=BTreeMap::new();
                                for ((d,_,_,_),tile) in &summary.detailed_data {for (at,kind) in &tile.portals {points.insert((*d,at[0],at[2],*kind),*at);}}
                                *portal_points.lock().unwrap_or_else(|e|e.into_inner())=points;
                                *residency.lock().unwrap_or_else(|e|e.into_inner())=summary.detailed.clone();
                                *coverage_state.lock().unwrap_or_else(|e|e.into_inner())=Arc::new(summary);
                                last_probe=Some(tag);
                            }
                        }
                        let target=prediction.lock().unwrap_or_else(|e|e.into_inner()).clone().filter(|v|v.session==view.session&&v.dimension==view.dimension&&v.layer==view.layer&&v.large==view.large).unwrap_or_else(||view.clone());
                        let before=cache.clock;
                        if !paused && let Err(e)=cache.presentation_step(&view,&target,shading.as_ref()) {
                            paused=true;
                            *errors.lock().unwrap_or_else(|e|e.into_inner())=Some(format!("Cache write failed; sampling paused: {e}"));
                        }
                        changed|=before!=cache.clock;
                        let same = result
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .as_ref()
                            .is_some_and(|f: &Arc<Frame>| f.view == view);
                        if same && !changed {
                            continue;
                        }
                        // Camera-only frames reuse immutable GPU data without scanning the
                        // disk index, touching residency or rebuilding coverage snapshots.
                        if !changed && let Some(ready) = &atlas
                            && ready.key == (view.session, view.dimension, view.layer, cache.clock)
                            && ready.terrain.covers(&view)
                        {
                            serial += 1;
                            *result.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(Frame {
                                raster_source: None,
                                serial,
                                known: ready.terrain.chunks.len() + ready.terrain.coarse.len(),
                                atlas: Some(ready.terrain.clone()),
                                view,
                                pixels: Vec::new(),
                            }));
                            continue;
                        }
                        serial += 1;
                        let frame = {
                            let terrain = cache.prepared_atlas(&view, &target, &mut atlas);
                            Arc::new(Frame {
                                raster_source: None,
                                serial,
                                view,
                                pixels: Vec::new(),
                                known: terrain.chunks.len() + terrain.coarse.len(),
                                atlas: Some(terrain),
                            })
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
                            cache.overviews.displayed|=frame.known>0;
                            *result.lock().unwrap_or_else(|e| e.into_inner()) = Some(frame);
                        }
                    }
            }).map_err(|e| eprintln!("Map worker failed: {e}")).ok();
            if worker.is_none() || sampler.is_none() {
                paused.store(true, Ordering::Release);
                *status.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some("Map worker unavailable; sampling paused".into());
            }
            Self {
                position,
                tx: Mutex::new(Some(tx)),
                worker: Mutex::new(worker),
                sampler: Mutex::new(sampler),
                lookahead,
                probe,
                wake: Mutex::new(Some(wake)),
                latest,
                output,
                session,
                resident,
                coverage,
                portals,
                binding,
                upgrade,
                status,
                paused,
            }
        }
        pub fn remember_position(&self, session: u64, position: LastPosition) {
            self.position
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(session, position);
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
                .map(|message| super::localize_message(&message))
        }
        pub fn needs_upgrade(&self, world: &str) -> bool {
            self.upgrade
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_deref()
                == Some(world)
        }
        pub fn delete_cache(
            &self,
            selection: container::DeleteSelection,
            active_world: &str,
        ) -> Result<mpsc::Receiver<Result<(), String>>, String> {
            let (reply, rx) = mpsc::sync_channel(1);
            let wake = self.wake.lock().unwrap_or_else(|e| e.into_inner());
            let tx = wake.as_ref().ok_or("Cache worker is unavailable")?;
            tx.try_send(Job::Delete {
                selection,
                active_world: active_world.into(),
                reply,
            })
            .map_err(|_| "Cache worker is busy; try again".to_owned())?;
            Ok(rx)
        }

        pub fn rebuild_images(
            &self,
            world: &str,
            file: &str,
        ) -> Result<mpsc::Receiver<Result<(), String>>, String> {
            if !super::valid_waypoint_world(world) {
                return Err("Unknown cache identity".into());
            }
            let (reply, rx) = mpsc::sync_channel(1);
            let wake = self.wake.lock().unwrap_or_else(|e| e.into_inner());
            let Some(tx) = wake.as_ref() else {
                return Err("Cache worker is unavailable".into());
            };
            tx.try_send(Job::Rebuild {
                world: world.into(),
                file: file.into(),
                reply,
            })
            .map_err(|_| "Cache worker is busy; try again".to_string())?;
            Ok(rx)
        }
        pub fn reset(&self, session: u64) {
            *self.binding.lock().unwrap_or_else(|e| e.into_inner()) = None;
            *self.latest.lock().unwrap_or_else(|e| e.into_inner()) = None;
            self.session.store(session, Ordering::Release);
            if let Some(tx) = self.tx.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                let _ = tx.try_send(Job::Wake);
            }
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
            self.request_ahead(view.clone(), view);
        }
        pub fn inspect(&self, session: u64, dimension: Dimension, layer: Layer, at: [i32; 2]) {
            let changed = {
                let mut probe = self.probe.lock().unwrap_or_else(|e| e.into_inner());
                let chunk = at.map(|v| v.div_euclid(16));
                let changed = probe.as_ref().is_none_or(|(s, d, l, p)| {
                    (*s, *d, *l, p.map(|v| v.div_euclid(16))) != (session, dimension, layer, chunk)
                });
                *probe = Some((session, dimension, layer, at));
                changed
            };
            if changed {
                self.wake();
            }
        }
        fn wake(&self) {
            if let Some(wake) = self.wake.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                let _ = wake.try_send(Job::Wake);
            }
        }
        pub fn request_ahead(&self, view: View, target: View) {
            let target_changed = {
                let mut ahead = self.lookahead.lock().unwrap_or_else(|e| e.into_inner());
                let changed = ahead.as_ref() != Some(&target);
                *ahead = Some(target);
                changed
            };
            let mut latest = self.latest.lock().unwrap_or_else(|e| e.into_inner());
            if latest.as_ref() == Some(&view) && !target_changed {
                return;
            }
            *latest = Some(view);
            drop(latest);
            self.wake();
        }
        pub fn frame(&self) -> Option<Arc<Frame>> {
            self.output
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        }
        pub fn shutdown(&self) {
            self.tx.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(sampler) = self
                .sampler
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
            {
                let _ = sampler.join();
            }
            self.wake.lock().unwrap_or_else(|e| e.into_inner()).take();
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
    super::super::minimap_maps_tests::define_tests!();
}

#[cfg(test)]
super::minimap_map_regression_tests::define_tests!();
