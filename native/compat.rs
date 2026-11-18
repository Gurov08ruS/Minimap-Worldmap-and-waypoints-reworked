// SPDX-License-Identifier: GPL-3.0-only
// Adapter to the published SDK. Chunk payloads remain shared immutable Arcs.
use super::Game;
use horizon_client::{chunk::Chunk, hud::HudBatch};
use horizon_world::registry::BlockStateId;
use std::{ops::Range, sync::Arc};

pub enum WorldId {
    Singleplayer(String),
    Server(String),
}
pub struct ClientState {
    pub world: Option<WorldId>,
    pub gui_size: [f32; 2],
}
pub struct BiomeId(pub u16);
pub struct Snapshot {
    origin: [i32; 2],
    width: usize,
    chunks: Vec<Option<Arc<Chunk>>>,
    height: Range<i32>,
    air: BlockStateId,
}
impl Snapshot {
    fn chunk(&self, x: i32, z: i32) -> Option<&Chunk> {
        let dx = i64::from(x >> 4) - i64::from(self.origin[0]);
        let dz = i64::from(z >> 4) - i64::from(self.origin[1]);
        if !(0..self.width as i64).contains(&dx) || dz < 0 {
            return None;
        }
        let index = usize::try_from(dz * self.width as i64 + dx).ok()?;
        self.chunks.get(index)?.as_deref()
    }
    pub fn height(&self) -> Range<i32> {
        self.height.clone()
    }
    pub fn block(&self, [x, y, z]: [i32; 3]) -> BlockStateId {
        if !self.height.contains(&y) {
            return self.air;
        }
        self.chunk(x, z)
            .map_or(self.air, |c| c.block(x & 15, y, z & 15))
    }
    pub fn biome(&self, [x, y, z]: [i32; 3]) -> Option<BiomeId> {
        self.chunk(x, z)
            .map(|c| BiomeId(c.biome(x, y.clamp(self.height.start, self.height.end - 1), z)))
    }
    pub fn section_top(&self, [x, z]: [i32; 2]) -> i32 {
        self.chunk(x, z).map_or(self.height.start, |c| {
            c.sections
                .iter()
                .rposition(|s| !s.is_uniform(self.air.0))
                .map_or(self.height.start, |s| c.min_y() + (s as i32 + 1) * 16)
        })
    }
}
pub trait MapAccess {
    fn map_client_state(&self) -> Option<ClientState>;
    fn map_chunks(&self, from: [i32; 2], to: [i32; 2]) -> Result<Snapshot, String>;
    fn map_biome(&self, pos: [i32; 3]) -> Option<String>;
    fn map_key(&self, mod_id: &str, name: &str, default: &str) -> String;
    fn map_text_width(&self, text: &str) -> f32;
    fn map_text(&self, batch: &mut HudBatch, text: &str, at: [f32; 2], color: u32, shadow: bool);
    fn map_sprite(&self, batch: &mut HudBatch, name: &str, src: [f32; 4], dst: [f32; 4]) -> bool;
    fn map_light(&self, pos: [i32; 3]) -> (u8, u8);
    fn map_on(&self, id: &str) -> bool;
}
impl MapAccess for Game<'_> {
    fn map_client_state(&self) -> Option<ClientState> {
        if !self.native_window() {
            return None;
        }
        let (w, h) = self.gui_size();
        let world = self.remote.as_ref().filter(|r| r.logged_in).and_then(|r| {
            match self.integrated.as_ref().map(|server| &server.save_folder) {
                Some(f) => Some(WorldId::Singleplayer(f.clone())),
                // A memory connection's "local" address does not identify a
                // world. Keep it transient rather than mixing guests' saves.
                None if r.session.address == "local" || r.session.address.is_empty() => None,
                None => Some(WorldId::Server(r.session.address.clone())),
            }
        });
        Some(ClientState {
            world,
            gui_size: [w, h],
        })
    }
    fn map_chunks(&self, from: [i32; 2], to: [i32; 2]) -> Result<Snapshot, String> {
        let lo = [from[0].min(to[0]), from[1].min(to[1])];
        let hi = [from[0].max(to[0]), from[1].max(to[1])];
        let count = (i64::from(hi[0]) - i64::from(lo[0]) + 1)
            .saturating_mul(i64::from(hi[1]) - i64::from(lo[1]) + 1);
        if count > 9 {
            return Err("map snapshot exceeds its 3x3 neighbourhood".into());
        }
        let mut chunks = Vec::with_capacity(count as usize);
        for z in lo[1]..=hi[1] {
            for x in lo[0]..=hi[0] {
                chunks.push(self.world.chunks.get(&(x, z)).cloned());
            }
        }
        Ok(Snapshot {
            origin: lo,
            width: (i64::from(hi[0]) - i64::from(lo[0]) + 1) as usize,
            chunks,
            height: self.world.shape.min_y..self.world.shape.max_y(),
            air: self.world.air,
        })
    }
    fn map_biome(&self, pos: [i32; 3]) -> Option<String> {
        let p = horizon_core::position::BlockPos::new(pos[0], pos[1], pos[2]);
        let [x, y, z] = self.streaming.biome_zoom.quart_at_block(p);
        self.biome_name_at_block([x << 2, y << 2, z << 2])
    }
    fn map_key(&self, mod_id: &str, name: &str, default: &str) -> String {
        self.mod_key(mod_id, name)
            .map_or_else(|| default.to_owned(), |(key, _)| key)
    }
    fn map_text_width(&self, text: &str) -> f32 {
        HudBatch::text_width(&self.gui.atlas, text)
    }
    fn map_text(&self, batch: &mut HudBatch, text: &str, at: [f32; 2], color: u32, shadow: bool) {
        self.native_text(
            batch,
            text,
            at,
            if color >> 24 == 0 {
                color | 0xff000000
            } else {
                color
            },
            shadow,
        );
    }
    fn map_sprite(&self, batch: &mut HudBatch, name: &str, src: [f32; 4], dst: [f32; 4]) -> bool {
        self.native_sprite(batch, name, src, dst, None)
    }
    fn map_light(&self, [x, y, z]: [i32; 3]) -> (u8, u8) {
        if !self.world.loaded(x, z) {
            return (0, 0);
        }
        let light = self.world.light(x, y, z);
        (light & 15, light >> 4)
    }
    fn map_on(&self, id: &str) -> bool {
        !self.settings.mods.disabled.contains(id)
    }
}

struct Synced {
    values: [Option<u64>; 21],
    preferences: ::minimap_worldmap_waypoints_reworked::runtime::minimap::Settings,
}
fn row_button(key: &str) -> Option<usize> {
    use ::minimap_worldmap_waypoints_reworked::runtime as rt;
    match key {
        "cache_ram_mib" => Some(18),
        "cache_manager" => Some(19),
        "reset" => Some(6),
        _ => rt::SDK_SETTING_ROWS
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, button)| *button),
    }
}
// Only changed preferences reach the SDK's persistence API. Ordinary frames
// compare fixed-size snapshots without filesystem access or allocations.
pub fn sync_settings(game: &mut Game<'_>) {
    use ::minimap_worldmap_waypoints_reworked::{self as entry, runtime as rt};
    use horizon_client::mod_list::NativeSettingValue as Value;
    static LAST: std::sync::OnceLock<std::sync::Mutex<Synced>> = std::sync::OnceLock::new();
    let controller = rt::initialize(game.native_data_dir(rt::MOD_ID));
    let mut sync = LAST
        .get_or_init(|| {
            let initial = entry::initial_settings();
            let mut values = [None; 21];
            for (key, button) in rt::SDK_SETTING_ROWS {
                let _ = key;
                let index = initial.option_index(button).unwrap_or(0);
                controller.set_option(button, index);
                values[button] = Some(index as u64);
            }
            controller.set_cache_ram_mib(initial.cache_ram_mib);
            values[18] = Some(f64::from(initial.cache_ram_mib).to_bits());
            std::sync::Mutex::new(Synced {
                values,
                preferences: initial,
            })
        })
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut values = sync.values;
    if let Some(page) = game
        .settings
        .mods
        .installed
        .iter()
        .find(|m| m.id == rt::MOD_ID)
        .and_then(|m| m.native.as_ref())
    {
        for row in &page.settings {
            if let Some(button) = row_button(&row.key) {
                values[button] = match &row.value {
                    Value::Choice { index, .. } => Some(*index as u64),
                    Value::Range { value, .. } => Some(value.to_bits()),
                    _ => None,
                };
            }
        }
    }
    if values == sync.values && controller.settings() == sync.preferences {
        return;
    }
    let mut from_sdk = [false; 21];
    let mut reset = false;
    let mut caches = false;
    for (button, value) in values.iter().enumerate() {
        if *value == sync.values[button] {
            continue;
        }
        let Some(value) = value else {
            continue;
        };
        from_sdk[button] = true;
        match button {
            6 => reset = sync.values[6].is_some(),
            19 => caches = sync.values[19].is_some(),
            18 => {
                controller.set_cache_ram_mib(f64::from_bits(*value).round().clamp(16., 512.) as u16)
            }
            _ => {
                controller.set_option(button, *value as usize);
            }
        }
    }
    if reset {
        controller.click(6);
    }
    let current = controller.settings();
    let mut writes = Vec::new();
    if current != sync.preferences {
        for (key, button) in rt::SDK_SETTING_ROWS {
            let selected = current.option_index(button).unwrap_or(0);
            if !from_sdk[button]
                && (reset || current.option_index(button) != sync.preferences.option_index(button))
            {
                writes.push((key, selected.to_string()));
            }
            values[button] = Some(selected as u64);
        }
        if !from_sdk[18] && (reset || current.cache_ram_mib != sync.preferences.cache_ram_mib) {
            writes.push(("cache_ram_mib", current.cache_ram_mib.to_string()));
        }
        values[18] = Some(f64::from(current.cache_ram_mib).to_bits());
    }
    if let Some(page) = game
        .settings
        .mods
        .installed
        .iter_mut()
        .find(|m| m.id == rt::MOD_ID)
        .and_then(|m| m.native.as_mut())
    {
        for row in &mut page.settings {
            if let Some(button) = row_button(&row.key) {
                match &mut row.value {
                    Value::Choice { index, .. } if button != 6 && button != 19 => {
                        *index = values[button].unwrap_or(0) as usize
                    }
                    Value::Range { value, .. } if button == 18 => {
                        *value = f64::from(current.cache_ram_mib)
                    }
                    _ => {}
                }
            }
        }
    }
    sync.values = values;
    sync.preferences = current;
    drop(sync);
    for (key, value) in writes {
        game.store_setting(rt::MOD_ID, key, &value);
    }
    if caches {
        super::reworked_maps::open_cache_manager(game);
    }
}
