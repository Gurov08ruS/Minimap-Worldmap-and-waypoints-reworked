// SPDX-License-Identifier: GPL-3.0-only
// Injected into Horizon's CPU LOD module. Read existing immutable samples only.
use std::{collections::BTreeMap, sync::Arc};

pub fn snapshots(
    lod: &super::Lod,
    known: &BTreeMap<(i32, i32, u8), usize>,
    focus: [f64; 2],
    limit: usize,
) -> Vec<(super::TileKey, Arc<Vec<super::Column>>)> {
    let mut available: Vec<_> = lod
        .ready
        .iter()
        .filter(|(_, columns)| {
            columns.len() == 256 && columns.iter().any(|column| column.surface.is_some())
        })
        .collect();
    available.sort_unstable_by(|(a, _), (b, _)| {
        let distance = |key: &super::TileKey| {
            let size = f64::from(key.size());
            let x = (focus[0] - (f64::from(key.x) + 0.5) * size).abs() - size / 2.0;
            let z = (focus[1] - (f64::from(key.z) + 0.5) * size).abs() - size / 2.0;
            (x.max(0.0).hypot(z.max(0.0)), key.level, key.x, key.z)
        };
        let a = distance(a);
        let b = distance(b);
        a.0.total_cmp(&b.0)
            .then_with(|| (a.1, a.2, a.3).cmp(&(b.1, b.2, b.3)))
    });
    available
        .into_iter()
        .filter(|(key, columns)| {
            known.get(&(key.x, key.z, key.level)).copied() != Some(Arc::as_ptr(columns) as usize)
        })
        .take(limit.min(4))
        .map(|(key, columns)| (*key, Arc::clone(columns)))
        .collect()
}
