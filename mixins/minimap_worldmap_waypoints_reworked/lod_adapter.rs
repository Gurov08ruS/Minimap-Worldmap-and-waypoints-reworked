// SPDX-License-Identifier: GPL-3.0-only
// Injected into Horizon's CPU LOD module. Read existing immutable samples only.
use std::{collections::BTreeMap, sync::Arc};

pub fn priority(
    x: i32,
    z: i32,
    level: u8,
    focus: [f64; 2],
    span: [f64; 2],
) -> (bool, std::cmp::Reverse<u8>, u64, i32, i32) {
    let size = (16_i64 << level.min(22)) as f64;
    let dx = (focus[0] - (f64::from(x) + 0.5) * size).abs() - size / 2.;
    let dz = (focus[1] - (f64::from(z) + 0.5) * size).abs() - size / 2.;
    (
        dx > span[0] / 2. || dz > span[1] / 2.,
        std::cmp::Reverse(level),
        (dx.max(0.).powi(2) + dz.max(0.).powi(2)) as u64,
        x,
        z,
    )
}

pub fn snapshots(
    lod: &horizon_client::lod::Lod,
    known: &BTreeMap<(i32, i32, u8), usize>,
    focus: [f64; 2],
    span: [f64; 2],
    limit: usize,
) -> Vec<(
    horizon_client::lod::TileKey,
    Arc<Vec<horizon_client::lod::Column>>,
)> {
    let limit = limit.min(8);
    if limit == 0 {
        return Vec::new();
    }
    let mut available: Vec<_> = lod
        .ready
        .iter()
        .filter(|(key, columns)| {
            known.get(&(key.x, key.z, key.level)).copied() != Some(Arc::as_ptr(columns) as usize)
        })
        .filter(|(_, columns)| {
            columns.len() == 256 && columns.iter().any(|column| column.surface.is_some())
        })
        .map(|(key, columns)| (priority(key.x, key.z, key.level, focus, span), key, columns))
        .collect();
    if available.len() > limit {
        available.select_nth_unstable_by_key(limit, |v| v.0);
        available.truncate(limit);
    }
    available.sort_unstable_by_key(|v| v.0);
    available
        .into_iter()
        .take(limit)
        .map(|(_, key, columns)| (*key, Arc::clone(columns)))
        .collect()
}
