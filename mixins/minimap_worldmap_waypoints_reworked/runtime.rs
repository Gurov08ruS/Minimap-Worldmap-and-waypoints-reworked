// SPDX-License-Identifier: GPL-3.0-only
// Based on horizon-rust/horizon-native-mod-template (MIT); see LICENSE-MIT and NOTICE.
// Shared by Cargo tests and the injected game module. Keep modules inline.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, mpsc},
    thread::{self, JoinHandle},
};

pub const MOD_ID: &str = "minimap_worldmap_waypoints_reworked";
pub const MOD_TITLE: &str = "Minimap, Worldmap and waypoints reworked";
pub const SETTINGS_FILE: &str = "settings.ini";
const SIZES: [u16; 7] = [64, 96, 128, 160, 192, 224, 256];
const ZOOMS: [f32; 7] = [0.1, 0.25, 0.5, 0.75, 1.0, 2.0, 4.0];

pub mod minimap {
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
                Self::TopLeft => "Слева сверху",
                Self::TopRight => "Справа сверху",
                Self::BottomLeft => "Слева снизу",
                Self::BottomRight => "Справа снизу",
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
                "# Minimap Reworked settings\nenabled={}\ncorner={}\nsize={}\nshape={}\nzoom={}\nrotate={}\n",
                self.enabled,
                self.corner.name(),
                self.size_px,
                if self.round { "round" } else { "square" },
                self.blocks_per_pixel,
                self.rotate_with_player,
            )
        }

        // Returns the changed field mask. Unknown buttons are harmless.
        fn click(&mut self, button: usize) -> u8 {
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
                    return 0x3f;
                }
                _ => return 0,
            }
            1 << button
        }

        // Only locally edited fields override settings loaded asynchronously.
        fn overlay(&mut self, edited: Self, mask: u8) {
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
            if mask & 32 != 0 {
                self.rotate_with_player = edited.rotate_with_player;
            }
        }

        pub fn labels(self) -> Vec<String> {
            let on = |v| if v { "Вкл" } else { "Выкл" };
            vec![
                format!("Миникарта: {}", on(self.enabled)),
                format!("Положение: {}", self.corner.label()),
                format!("Размер: {} GUI px", self.size_px),
                format!(
                    "Форма: {}",
                    if self.round {
                        "Круг"
                    } else {
                        "Квадрат"
                    }
                ),
                format!("Масштаб: {} бл./пикс.", self.blocks_per_pixel),
                format!("Вращение: {}", on(self.rotate_with_player)),
                "Сбросить настройки миникарты".into(),
            ]
        }

        pub fn fitted_size(self, width: f32, height: f32) -> Option<f32> {
            // Horizon clamps its minimap to at least 16 GUI pixels. Hide if that cannot fit.
            let available = (width - 16.0).min(height - 64.0);
            (available >= 16.0).then(|| f32::from(self.size_px).min(available))
        }
    }

    pub(super) fn apply(settings: &mut Settings, button: usize) -> u8 {
        settings.click(button)
    }
    pub(super) fn merge(settings: &mut Settings, edited: Settings, mask: u8) {
        settings.overlay(edited, mask);
    }
}

use minimap::Settings;

fn cycle<T: Copy + PartialEq>(current: T, choices: &[T]) -> T {
    let index = choices.iter().position(|v| *v == current).unwrap_or(0);
    choices[(index + 1) % choices.len()]
}

#[derive(Default)]
struct State {
    settings: Settings,
    touched: u8,
    revision: u64,
    error: Option<String>,
}

// One bounded notification queue and one settings worker; no filesystem access in HUD callbacks.
pub struct Controller {
    state: Arc<Mutex<State>>,
    notify: Mutex<Option<mpsc::SyncSender<()>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
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
            labels.push("Ошибка сохранения настроек: см. журнал клиента".into());
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

    // Called on application exit, never during drawing. Drain accepted edits before returning.
    pub fn shutdown(&self) {
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
    let loaded = read_settings(&path);
    {
        let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
        match loaded {
            Ok(mut settings) => {
                minimap::merge(&mut settings, state.settings, state.touched);
                state.settings = settings;
            }
            Err(error) => {
                eprintln!("[{MOD_ID}] Cannot read {}: {error}", path.display());
                state.error = Some(error.to_string());
            }
        }
    }
    let mut saved_revision = 0;
    while rx.recv().is_ok() {
        persist_latest(&path, shared, &mut saved_revision);
    }
    // Retry a dirty last revision once at normal exit after a transient failure.
    persist_latest(&path, shared, &mut saved_revision);
}

fn persist_latest(path: &Path, shared: &Mutex<State>, saved_revision: &mut u64) {
    let (settings, revision) = {
        let state = shared.lock().unwrap_or_else(|e| e.into_inner());
        (state.settings, state.revision)
    };
    if revision <= *saved_revision {
        return;
    }
    let result = write_settings(path, settings, revision);
    if result.is_ok() {
        *saved_revision = revision;
    }
    shared.lock().unwrap_or_else(|e| e.into_inner()).error = result.err().map(|error| {
        eprintln!("[{MOD_ID}] Cannot save {}: {error}", path.display());
        format!("Save: {error}")
    });
}

fn read_settings(path: &Path) -> io::Result<Settings> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(error) => return Err(error),
    };
    let mut text = String::new();
    file.take(4097).read_to_string(&mut text)?;
    if text.len() > 4096 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Settings file exceeds 4 KiB",
        ));
    }
    Ok(Settings::decode(&text))
}

fn write_settings(path: &Path, settings: Settings, revision: u64) -> io::Result<()> {
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
        file.write_all(settings.encode().as_bytes())?;
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
        assert!(controller.labels().last().unwrap().contains("Ошибка"));
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
