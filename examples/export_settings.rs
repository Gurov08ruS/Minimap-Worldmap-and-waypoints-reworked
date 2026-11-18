// SPDX-License-Identifier: GPL-3.0-only
//! Regenerate the SDK label catalog from the shared runtime's translations.
use minimap_worldmap_waypoints_reworked as rt;
fn line(language: &str, key: &str, value: &str) {
    assert!(!value.contains(['\t', '\n']));
    println!("{language}\t{key}\t{value}");
}
fn main() {
    for language in ["en_us", "ru_ru", "fr_fr", "es_es", "de_de"] {
        rt::set_language(language);
        let base = rt::minimap::Settings::default();
        for (key, button) in rt::SDK_SETTING_ROWS {
            let label = if button == 20 {
                rt::tr(
                    "Waypoint initials",
                    "Буквы вейпоинтов",
                    "Initiales des repères",
                    "Iniciales de marcadores",
                    "Wegpunktinitialen",
                )
                .to_owned()
            } else {
                base.labels()[button].split_once(':').map_or_else(
                    || base.labels()[button].clone(),
                    |(name, _)| name.to_owned(),
                )
            };
            line(language, &format!("mwwr.setting.{key}"), &label);
            for (index, option) in rt::minimap::Settings::options(button).iter().enumerate() {
                line(
                    language,
                    &format!("mwwr.value.{key}.{index}"),
                    &option.option_label(button).unwrap(),
                );
            }
        }
        for (key, value) in [
            (
                "mwwr.setting.cache_ram_mib",
                rt::tr(
                    "Map RAM (MiB)",
                    "RAM карты (МиБ)",
                    "RAM carte (Mio)",
                    "RAM del mapa (MiB)",
                    "Karten-RAM (MiB)",
                ),
            ),
            (
                "mwwr.setting.cache_manager",
                rt::tr(
                    "Cache (in world)",
                    "Кеш (в мире)",
                    "Cache (en jeu)",
                    "Caché (en el mundo)",
                    "Cache (im Spiel)",
                ),
            ),
            (
                "mwwr.action.cache_manager",
                rt::tr(
                    "Manage…",
                    "Менеджер…",
                    "Gérer…",
                    "Administrar…",
                    "Verwalten…",
                ),
            ),
            (
                "mwwr.setting.reset",
                rt::tr(
                    "Preferences",
                    "Настройки",
                    "Préférences",
                    "Preferencias",
                    "Einstellungen",
                ),
            ),
            (
                "mwwr.action.reset",
                rt::tr(
                    "Reset…",
                    "Сбросить…",
                    "Réinitialiser…",
                    "Restablecer…",
                    "Zurücksetzen…",
                ),
            ),
            (
                "mwwr.key.world_map",
                rt::tr(
                    "Open world map",
                    "Открыть большую карту",
                    "Ouvrir la carte du monde",
                    "Abrir mapa del mundo",
                    "Weltkarte öffnen",
                ),
            ),
            (
                "mwwr.key.waypoints",
                rt::tr(
                    "Waypoints",
                    "Вейпоинты",
                    "Repères",
                    "Marcadores",
                    "Wegpunkte",
                ),
            ),
        ] {
            line(language, key, value);
        }
    }
}
