// SPDX-License-Identifier: GPL-3.0-only
// Child of hud: use Horizon's portable, per-renderer GUI picture
// texture ownership rather than reserving or overwriting shared atlas slots.
use super::pictures::Picture;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

pub fn create(width: u32, height: u32, pixels: Vec<u8>) -> Arc<Picture> {
    // Resource pictures allocate upward from 1. Native map pages own the high
    // half of the ID space and never reuse an ID across resource reloads.
    static NEXT: AtomicU64 = AtomicU64::new(1 << 63);
    Arc::new(Picture {
        id: NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .expect("native map picture ID exhaustion"),
        width,
        height,
        pixels,
        flags: super::TextureFlags {
            blur: false,
            clamp: true,
        },
    })
}

pub fn paint(batch: &mut super::HudBatch, picture: &Arc<Picture>, points: &[[f64; 4]]) {
    if points.len() < 3 {
        return;
    }
    let start = batch.vertices.len();
    for i in 1..points.len() - 1 {
        let p = [points[0], points[i], points[i + 1], points[i + 1]];
        batch.quad(
            p.map(|v| [v[0] as f32, v[1] as f32]),
            p.map(|v| {
                [
                    v[2] as f32 / picture.width as f32,
                    v[3] as f32 / picture.height as f32,
                ]
            }),
            u32::MAX,
        );
    }
    batch.blend_since(start, super::Blend::Picture(picture.id));
    if !batch.pictures.iter().any(|p| p.id == picture.id) {
        batch.pictures.push(picture.clone());
    }
}
