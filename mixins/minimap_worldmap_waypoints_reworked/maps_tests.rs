// SPDX-License-Identifier: GPL-3.0-only
// Expand at the original scope to retain private access and test names.
// Mixins requires self-contained payloads; external mod/include files are unsupported.

#[cfg(test)]
macro_rules! define_tests {
    () => {
        mod tests {
            #[test]
            fn prepared_pages_survive_payload_eviction_without_losing_world_coverage() {
                let mut cache=Cache::default();let d=Dimension::Overworld;let v=view(d);let key=LodKey{x:0,z:0,level:16};
                cache.insert_lod(d,key,coarse_tile([30,100,80]));cache.save(lod_record(d,key),cache.lod_tiles[&(d,key)].1.clone()).unwrap();finish_overviews(&mut cache,&v);
                let first=cache.visual_atlas(&v,&v,None);
                cache.overviews.tiles.clear();cache.overviews.order.clear();cache.clear_payloads();
                let retained=cache.visual_atlas(&v,&v,Some(&first));
                assert_eq!(retained.pixels,first.pixels);assert_eq!(retained.prepared_tiles,0);
                assert!(retained.pages.iter().zip(&first.pages).all(|(a,b)|Arc::ptr_eq(a,b)));
                assert!(retained.coarse.iter().any(|draw|draw.fragments.iter().any(|r|intersects(*r,[10000,10000,16,16]))));
                let mut other=v.clone();other.session+=1;
                assert!(cache.visual_atlas(&other,&other,Some(&first)).coarse.is_empty());
            }
            #[test]
            #[ignore="manual persistent visual-cache latency; warm OS file cache, not GPU FPS"]
            fn benchmark_persistent_zoom() {
                let dir=CacheDir::new("zoom-timing");let d=Dimension::Overworld;
                let mut cache=Cache::open(&dir.0,"save:zoom",64*1024*1024).unwrap();
                for x in -32..32 {for z in -32..32 {save_chunk(&mut cache,d,Layer::Surface,[x,z],Tile{pixels:[70,120,40,255].repeat(256),heights:[32;256],..Tile::default()});}}
                for x in -1..=0 {for z in -1..=0 {let k=LodKey{x,z,level:12};cache.insert_lod(d,k,coarse_tile([60,100,90]));cache.save(lod_record(d,k),cache.lod_tiles[&(d,k)].1.clone()).unwrap();}}
                let mut far=view(d);far.center=[0.;2];far.span=[100000.;2];far.width=1024;far.height=1024;
                let start=Instant::now();cache.presentation_step(&far,&far,None).unwrap();let preview=cache.visual_atlas(&far,&far,None);let first=start.elapsed();assert!(!preview.pages.is_empty());
                let start=Instant::now();finish_overviews(&mut cache,&far);cache.flush().unwrap();let build=start.elapsed();drop(cache);
                let start=Instant::now();let mut cache=Cache::open(&dir.0,"save:zoom",64*1024*1024).unwrap();finish_overviews(&mut cache,&far);let overview=cache.visual_atlas(&far,&far,None);let cold=start.elapsed();cache.overviews.displayed=true;
                let near=view(d);let start=Instant::now();
                loop {cache.presentation_step(&far,&near,None).unwrap();if cache.ready_level(&near)==0{break;}assert!(start.elapsed()<Duration::from_secs(10));}
                let prepared=cache.visual_atlas(&far,&near,Some(&overview));let ready=start.elapsed();assert!(prepared.chunks.contains(&[0,0]));
                let start=Instant::now();let sharp=cache.visual_atlas(&near,&near,Some(&prepared));let zoom=start.elapsed();assert!(sharp.chunks.contains(&[0,0]));
                eprintln!("canonical_tiles=4096 first_preview_ms={:.3} initial_backfill_ms={:.3} reopen_visual_ms={:.3} target_prefetch_ms={:.3} zoom_assembly_ms={:.3} pages={}",first.as_secs_f64()*1000.,build.as_secs_f64()*1000.,cold.as_secs_f64()*1000.,ready.as_secs_f64()*1000.,zoom.as_secs_f64()*1000.,sharp.pages.len());
            }
            fn finish_overviews(cache:&mut Cache,view:&View) {
                let start=Instant::now();
                loop {
                    cache.presentation_step(view,view,None).unwrap();
                    if !cache.visual_work() {break;}
                    assert!(start.elapsed()<Duration::from_secs(10),"visual backfill did not converge");
                }
            }
            #[test]
            fn derived_damage_is_repaired_and_metadata_probe_keeps_texture_identity() {
                let dir=CacheDir::new("derived-repair");let d=Dimension::Overworld;let v=view(d);
                let mut cache=Cache::open(&dir.0,"save:repair",64*1024*1024).unwrap();
                save_chunk(&mut cache,d,Layer::Surface,[0,0],saved_tile());finish_overviews(&mut cache,&v);cache.flush().unwrap();
                let key=overview_key(d,Layer::Surface,LodKey{x:0,z:0,level:0});
                cache.backing.as_ref().unwrap().borrow_mut().store.put(key,vec![0;32]).unwrap();cache.flush().unwrap();drop(cache);
                let mut cache=Cache::open(&dir.0,"save:repair",64*1024*1024).unwrap();
                finish_overviews(&mut cache,&v);assert!(cache.overview_get(key).is_some());cache.flush().unwrap();drop(cache);
                let service=Service::with_directory(Some(dir.0.clone()));service.reset(v.session);service.configure(v.session,"save:repair",d,64);service.request(v.clone());
                let frame=wait_frame(&service,|f|f.atlas.as_ref().is_some_and(|a|a.chunks.contains(&[0,0])));
                service.inspect(v.session,d,Layer::Surface,[0,0]);
                let start=Instant::now();
                while service.coverage().detailed_biome(d,Layer::Surface,[0,0]).is_none(){assert!(start.elapsed()<Duration::from_secs(5));std::thread::yield_now();}
                assert_eq!(service.coverage().detailed_biome(d,Layer::Surface,[0,0]).as_deref(),Some("minecraft:forest"));
                assert!(Arc::ptr_eq(frame.atlas.as_ref().unwrap(),service.frame().unwrap().atlas.as_ref().unwrap()));
                service.shutdown();
            }
            #[test]
            fn assembling_a_far_atlas_does_not_read_the_container() {
                let dir=CacheDir::new("visual-no-io");let d=Dimension::Overworld;let mut v=view(d);v.span=[100000.;2];
                let mut cache=Cache::open(&dir.0,"save:no-io",64*1024*1024).unwrap();let key=LodKey{x:0,z:0,level:16};
                cache.insert_lod(d,key,coarse_tile([30,100,80]));cache.save(lod_record(d,key),cache.lod_tiles[&(d,key)].1.clone()).unwrap();finish_overviews(&mut cache,&v);cache.flush().unwrap();
                let before=cache.backing.as_ref().unwrap().borrow().clock;
                let first=cache.visual_atlas(&v,&v,None);let second=cache.visual_atlas(&v,&v,Some(&first));
                assert_eq!(before,cache.backing.as_ref().unwrap().borrow().clock);
                assert_eq!(second.prepared_tiles,0);
                assert!(first.pages.iter().zip(&second.pages).all(|(a,b)|Arc::ptr_eq(a,b)));
                assert!(first.coarse.iter().any(|draw|draw.fragments.iter().any(|r|intersects(*r,[10000,10000,16,16]))));
            }
            #[test]
            fn overview_reduction_preserves_colour_coverage_and_known_void() {
                let mut source=Overview::default();
                source.pixels[..4].copy_from_slice(&[255,80,20,255]); source.coverage[0]=u16::MAX;
                let reduced=Overview::reduce(&[Some(Arc::new(source)),None,None,None]);
                assert_eq!(&reduced.pixels[..4],&[255,80,20,64]);
                assert_eq!(reduced.coverage[0],16384);
                let mut source=Overview::default();
                for i in [0,1,16,17] {source.coverage[i]=u16::MAX;}
                let mut reduced=Overview::reduce(&[Some(Arc::new(source)),None,None,None]);
                reduced.fill(0,&[0,0,255,255]);
                assert_eq!(&reduced.pixels[..4],&[0,0,0,0]);
                assert_eq!(reduced.coverage[0],u16::MAX);
                assert_eq!(Overview::decode(&reduced.encode()).unwrap().pixels,reduced.pixels);
                assert!(Overview::decode(&[0;1544]).is_none());
            }
            #[test]
            fn overview_pages_cut_lod_out_of_known_void_at_negative_coordinates() {
                let mut cache=Cache::default();
                let d=Dimension::Overworld; let layer=Layer::Surface;
                let mut empty=saved_tile();empty.pixels.fill(0);
                cache.insert(d,layer,[-1,-1],empty);
                cache.save(record_key(d,layer,-1,-1),cache.tiles[&(d,layer,-1,-1)].1.clone()).unwrap();
                let lod=LodKey{x:-1,z:-1,level:8}; cache.insert_lod(d,lod,coarse_tile([0,0,255]));
                cache.save(lod_record(d,lod),cache.lod_tiles[&(d,lod)].1.clone()).unwrap();
                let mut v=view(d);v.center=[-8.;2];
                finish_overviews(&mut cache,&v);
                let atlas=cache.visual_atlas(&v,&v,None);
                let at=atlas.chunks.iter().position(|c|*c==[-1,-1]).unwrap();let slot=atlas.slots[at];
                let start=(slot/64*16*1024+slot%64*16)*4;
                assert_eq!(&atlas.pixels[start..start+4],&[0,0,0,0]);
                for draw in &atlas.coarse {for fragment in &draw.fragments {assert!(!intersects(*fragment,[-16,-16,16,16]));}}
                assert!(!atlas.pages.is_empty());
            }
            #[test]
            fn visual_records_reopen_sharp_without_loading_canonical_payloads() {
                let dir=CacheDir::new("persistent-visual");let d=Dimension::Overworld;
                let mut cache=Cache::open(&dir.0,"save:visual",64*1024*1024).unwrap();
                for x in -8..8 {for z in -8..8 {save_chunk(&mut cache,d,Layer::Surface,[x,z],saved_tile());}}
                let v=view(d);finish_overviews(&mut cache,&v);cache.flush().unwrap();drop(cache);
                let mut cache=Cache::open(&dir.0,"save:visual",64*1024*1024).unwrap();
                assert!(!cache.overviews.stale);assert!(cache.overviews.done);
                finish_overviews(&mut cache,&v);
                let atlas=cache.visual_atlas(&v,&v,None);
                assert!(atlas.chunks.contains(&[0,0]));
                assert_eq!(cache.backing.as_ref().unwrap().borrow().clock,0,"ready visuals must not decode heights/biomes again");
                assert!(cache.overviews.dirty.is_empty());
            }
            #[test]
            fn dirty_visual_checkpoint_resumes_and_detects_an_older_writer() {
                let dir=CacheDir::new("visual-checkpoint");let d=Dimension::Overworld;let key=record_key(d,Layer::Surface,0,0);
                let mut cache=Cache::open(&dir.0,"save:visual",64*1024*1024).unwrap();
                save_chunk(&mut cache,d,Layer::Surface,[0,0],saved_tile());let v=view(d);
                finish_overviews(&mut cache,&v);cache.flush().unwrap();
                let mut changed=saved_tile();changed.pixels=[1,240,10,255].repeat(256);
                save_chunk(&mut cache,d,Layer::Surface,[0,0],changed.clone());cache.flush().unwrap();drop(cache);
                let mut cache=Cache::open(&dir.0,"save:visual",64*1024*1024).unwrap();
                assert!(!cache.overviews.dirty.is_empty());finish_overviews(&mut cache,&v);cache.flush().unwrap();drop(cache);
                let mut store=Store::open(&dir.0,"save:visual").unwrap();store.put(key,tile_encode(&saved_tile()).unwrap()).unwrap();store.commit().unwrap();drop(store);
                let mut cache=Cache::open(&dir.0,"save:visual",64*1024*1024).unwrap();assert!(cache.overviews.stale);
                finish_overviews(&mut cache,&v);
                assert_eq!(cache.overview_get(overview_key(d,Layer::Surface,LodKey{x:0,z:0,level:0})).unwrap().pixels,cache.shaded_chunk(d,Layer::Surface,[0,0]));
            }
            #[test]
            fn far_view_uses_world_tiles_and_zoom_target_loads_original_texels() {
                let mut cache=Cache::default();let d=Dimension::Overworld;
                for x in -16..16 {for z in -16..16 {cache.insert(d,Layer::Surface,[x,z],saved_tile());cache.save(record_key(d,Layer::Surface,x,z),cache.tiles[&(d,Layer::Surface,x,z)].1.clone()).unwrap();}}
                let mut far=view(d);far.span=[100000.;2];far.width=1024;far.height=1024;
                finish_overviews(&mut cache,&far);
                let overview=cache.visual_atlas(&far,&far,None);assert!(!overview.coarse.is_empty());assert!(!overview.pages.is_empty());
                let near=view(d);
                cache.presentation_step(&far,&near,None).unwrap();
                let prefetched=cache.visual_atlas(&far,&near,Some(&overview));assert!(prefetched.chunks.contains(&[0,0]));
                let sharp=cache.visual_atlas(&near,&near,Some(&prefetched));let i=sharp.chunks.iter().position(|c|*c==[0,0]).unwrap();let slot=sharp.slots[i];let from=(slot/64*16*1024+slot%64*16)*4;
                assert_eq!(&sharp.pixels[from..from+64],&cache.shaded_chunk(d,Layer::Surface,[0,0])[..64]);
                assert!(sharp.prepared_tiles<=prefetched.prepared_tiles+32);
            }

            #[test]
            #[ignore = "manual CPU preparation timing; not GPU execution or game FPS"]
            fn benchmark_incremental_atlas_pages() {
                let mut cache = Cache::default();
                for x in -64..64 { for z in -32..32 {
                    cache.insert(Dimension::Overworld, Layer::Surface, [x,z], Tile {
                        pixels: [80,120,40,255].repeat(256), heights: [42;256], ..Tile::default()
                    });
                }}
                let mut v = view(Dimension::Overworld);
                v.center = [0.;2]; v.span = [4096.,2048.];
                let started = Instant::now();
                let first = cache.update_terrain_atlas(&v, None);
                let full = started.elapsed();
                let started = Instant::now();
                let warm = cache.update_terrain_atlas(&v, Some(&first));
                let reused = started.elapsed();
                assert_eq!(warm.prepared_tiles, 0);
                cache.insert(Dimension::Overworld, Layer::Surface, [0,0], Tile {
                    pixels: [30,100,90,255].repeat(256), heights: [45;256], ..Tile::default()
                });
                let shading = Shading::new().unwrap();
                let started = Instant::now();
                let changed = cache.prepare_terrain_atlas(&v, Some(&warm), Some(&shading));
                let update = started.elapsed();
                assert_eq!(changed.prepared_tiles, 3);
                let changed_pages = changed.pages.iter().zip(&warm.pages).filter(|(a,b)| !Arc::ptr_eq(a,b)).count();
                assert!(changed_pages <= 3);
                eprintln!("tiles={} pages={} full_us={} unchanged_us={} one_edit_us={} shaded={} changed_pages={}",
                    first.chunks.len(), first.pages.len(), full.as_micros(), reused.as_micros(), update.as_micros(), changed.prepared_tiles, changed_pages);
                super::super::profiling::report();
            }

            #[test]
            fn incremental_atlas_retains_slots_and_updates_relief_dependants() {
                let mut cache = Cache::default();
                for z in -1..=1 { for x in -1..=1 {
                    cache.insert(Dimension::Overworld, Layer::Surface, [x, z], saved_tile());
                }}
                let v = view(Dimension::Overworld);
                let first = cache.update_terrain_atlas(&v, None);
                assert_eq!(first.prepared_tiles, 9);
                let same = cache.update_terrain_atlas(&v, Some(&first));
                assert_eq!(same.prepared_tiles, 0);
                let mut changed = saved_tile();
                changed.heights.fill(50);
                changed.pixels.fill(100);
                cache.insert(Dimension::Overworld, Layer::Surface, [0, 0], changed);
                let next = cache.update_terrain_atlas(&v, Some(&same));
                assert_eq!(next.prepared_tiles, 3);
                assert_eq!(next.slots, same.slots);
                for (i, &chunk) in next.chunks.iter().enumerate() {
                    let expected = cache.shaded_chunk(v.dimension, v.layer, chunk);
                    let slot = next.slots[i];
                    for row in 0..16 {
                        let at = ((slot / 64 * 16 + row) * next.width + slot % 64 * 16) * 4;
                        assert_eq!(&next.pixels[at..at+64], &expected[row*64..row*64+64]);
                    }
                }
                cache.insert(Dimension::Overworld, Layer::Surface, [-2, 0], saved_tile());
                let expanded = cache.update_terrain_atlas(&v, Some(&next));
                for (&key, &slot) in &next.locations { assert_eq!(expanded.locations[&key], slot); }
            }

            #[test]
            fn native_pages_reuse_unchanged_images_beyond_the_shared_slot() {
                let mut cache = Cache::default();
                let mut v = view(Dimension::Overworld);
                v.span = [2048.; 2];
                for x in -33..33 { for z in -33..33 {
                    cache.insert(Dimension::Overworld, Layer::Surface, [x, z], saved_tile());
                }}
                assert!(cache.gpu_atlas_fits(&v));
                let first = cache.update_terrain_atlas(&v, None);
                assert!(first.height > 1024 && first.pages.len() > 16);
                assert!(first.pages.iter().all(|p| p.width <= 1024 && p.height <= 64));
                assert!(first.draw_order.windows(2).all(|p| first.slots[p[0]]/256 <= first.slots[p[1]]/256));
                let next = cache.update_terrain_atlas(&v, Some(&first));
                assert_eq!(next.prepared_tiles, 0);
                assert!(first.pages.iter().zip(&next.pages).all(|(a,b)| Arc::ptr_eq(a,b)));
                let mut tile = saved_tile(); tile.pixels[..4].copy_from_slice(&[1,2,3,255]);
                cache.insert(v.dimension, v.layer, [0,0], tile);
                let changed = cache.update_terrain_atlas(&v, Some(&next));
                assert!(next.pages.iter().zip(&changed.pages).filter(|(a,b)| !Arc::ptr_eq(a,b)).count() <= 3);
                assert_ne!(changed.pixels, next.pixels);
            }

            #[test]
            fn unreadable_detail_does_not_hide_valid_lod_in_gpu_pages() {
                let dir = CacheDir::new("bad-detail-visual");
                let mut cache = Cache::open(&dir.0, "save:bad-detail-visual", 1024*1024).unwrap();
                let key = LodKey { x:0, z:0, level:1 };
                cache.insert_lod(Dimension::Overworld, key, coarse_tile([20,40,60]));
                cache.save(lod_record(Dimension::Overworld,key), cache.lod_tiles[&(Dimension::Overworld,key)].1.clone()).unwrap();
                cache.backing.as_ref().unwrap().borrow_mut().store.put(record_key(Dimension::Overworld,Layer::Surface,0,0), vec![1;8]).unwrap();
                cache.flush().unwrap();
                cache.clear_payloads();
                let atlas = cache.update_terrain_atlas(&view(Dimension::Overworld), None);
                assert!(atlas.chunks.is_empty());
                assert_eq!(atlas.coarse.len(),1);
                assert!(atlas.coarse[0].fragments.iter().any(|r| r[0] <= 0 && r[1] <= 0 && r[0]+r[2] > 0 && r[1]+r[3] > 0));
                assert!(cache.backing.as_ref().unwrap().borrow().error.is_some());
            }

            #[test]
            fn predictive_coverage_is_bounded_and_never_crosses_sessions() {
                let v = view(Dimension::Overworld);
                let mut target = v.clone();
                target.center = [1e9, -1e9]; target.span = [1e8; 2];
                let ahead = v.predictive(&target);
                for axis in 0..2 {
                    assert!((ahead.center[axis] - v.center[axis]).abs() <= v.span[axis]/8.);
                    assert!(ahead.span[axis] <= v.span[axis]*1.5);
                }
                target.session += 1;
                assert_eq!(v.predictive(&target), v);
            }

            #[test]
            fn slow_sampling_does_not_block_camera_publication() {
                let service = Service::new();
                let mut v = view(Dimension::Overworld);
                v.session = 77;
                service.reset(77);
                service.request(v.clone());
                let first = wait_frame(&service, |_| true);
                let (entered, started) = std::sync::mpsc::channel();
                let (release, wait) = std::sync::mpsc::channel();
                assert!(service.submit(77, v.dimension, v.layer, [0,0], move || {
                    entered.send(()).unwrap(); wait.recv().unwrap(); saved_tile()
                }));
                started.recv_timeout(Duration::from_secs(5)).unwrap();
                v.center[0] += 4.;
                service.request(v.clone());
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| wait_frame(&service, |f| f.serial > first.serial && f.view == v)));
                release.send(()).unwrap();
                assert!(result.is_ok());
                service.shutdown();
            }

            impl Cache {
                fn update_terrain_atlas(&self, view: &View, previous: Option<&TerrainAtlas>) -> TerrainAtlas {
                    self.prepare_terrain_atlas(view, previous, None)
                }
                fn terrain_atlas(&self, view: &View, _serial: u64) -> TerrainAtlas {
                    self.update_terrain_atlas(view, None)
                }
            }
            #[test]
            fn disk_versions_reuse_visuals_after_payload_eviction_and_commit() {
                let dir = CacheDir::new("visual-versions");
                let mut cache = Cache::open(&dir.0, "save:visual-versions", 1024*1024).unwrap();
                save_chunk(&mut cache, Dimension::Overworld, Layer::Surface, [0,0], saved_tile());
                let v = view(Dimension::Overworld);
                let first = cache.update_terrain_atlas(&v, None);
                cache.flush().unwrap();
                cache.clear_payloads();
                let before = cache.backing.as_ref().unwrap().borrow().clock;
                let next = cache.update_terrain_atlas(&v, Some(&first));
                assert_eq!(next.prepared_tiles, 0);
                assert_eq!(before, cache.backing.as_ref().unwrap().borrow().clock);
                assert_eq!(next.pixels, first.pixels);
                let shading = Shading::new().unwrap();
                let threaded = cache.prepare_terrain_atlas(&v, None, Some(&shading));
                assert_eq!(threaded.pixels, first.pixels);
            }
            fn sample(
                block: impl FnMut(i32, i32, i32) -> Block,
                top: impl Fn(i32, i32) -> i32,
                chunk: [i32; 2],
                minimum: i32,
                layer: Layer,
            ) -> Tile {
                sample_with_biomes(block, top, chunk, minimum, layer, |_| None)
            }

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
                assert!(cache.gpu_atlas_fits(&v));
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
            fn minimap_camera_rotation_reuses_the_uploaded_terrain() {
                let service = Service::new();
                service.reset(1);
                let mut v = view(Dimension::Overworld);
                v.large = false;
                v.round = true;
                service.request(v.clone());
                assert!(service.submit(1, Dimension::Overworld, Layer::Surface, [-1, 0], saved_tile));
                let first = wait_frame(&service, |f| {
                    f.atlas
                        .as_ref()
                        .is_some_and(|a| a.chunks.contains(&[-1, 0]))
                });
                v.angle = 1.2;
                v.center[0] += 4.;
                service.request(v.clone());
                let rotated = wait_frame(&service, |f| f.view == v);
                assert!(Arc::ptr_eq(
                    first.atlas.as_ref().unwrap(),
                    rotated.atlas.as_ref().unwrap()
                ));
                assert!(rotated.pixels.is_empty());
                service.shutdown();
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
                let old = wait_frame(&service, |f| {
                    f.atlas
                        .as_ref()
                        .is_some_and(|a| a.chunks.contains(&[-1, 0]))
                });
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
                assert!(
                    wait_frame(&reopened, |f| f.known > 0)
                        .atlas
                        .as_ref()
                        .unwrap()
                        .chunks
                        .contains(&[-1, 0])
                );
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
                drop(wait_frame(&anonymous, |f| {
                    f.atlas
                        .as_ref()
                        .is_some_and(|a| a.chunks.contains(&[-1, 0]))
                }));
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
                assert!(cache.backing.as_ref().unwrap().borrow().store.pending_bytes() >= pending);
                assert_eq!(cache.backing.as_ref().unwrap().borrow_mut().store.read(record_key(Dimension::Overworld,Layer::Surface,0,0)).unwrap().unwrap(),tile_encode(&saved_tile()).unwrap());
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
            fn partial_uploads_reproduce_repacked_pixels_and_clear_old_tiles() {
                let width = 1024;
                let height = 64;
                let original = vec![7; width * height * 4];
                let mut next = original.clone();
                for row in 16..32 {
                    next[(row * width + 48) * 4..(row * width + 64) * 4].fill(0);
                }
                let patches = image_patch_regions(&original, &next, width, height);
                assert_eq!(patches, [[48, 16, 16, 16]]);
                let mut gpu = original.clone();
                for [x, y, w, h] in patches {
                    for row in y..y + h {
                        let start = (row * width + x) * 4;
                        gpu[start..start + w * 4].copy_from_slice(&next[start..start + w * 4]);
                    }
                }
                assert_eq!(gpu, next);
                assert!(image_patch_regions(&next, &next, width, height).is_empty());
                assert_eq!(
                    image_patch_regions(&[], &next, width, height),
                    [[0, 0, width, height]]
                );
                next.fill(2);
                assert_eq!(
                    image_patch_regions(&original, &next, width, height),
                    [[0, 0, width, height]]
                );
            }
            #[test]
            fn disk_lru_touch_and_eviction_preserve_the_recent_tile() {
                let dir = CacheDir::new("read-order");
                let mut cache = Cache::open(&dir.0, "save:read-order", 1024 * 1024).unwrap();
                for x in 0..3 {
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
                let mut b = cache.backing.as_ref().unwrap().borrow_mut();
                let size = tile_bytes(&tile_decode(&tile_encode(&saved_tile()).unwrap()).unwrap());
                b.limit = size * 2;
                let keys: [_; 3] = std::array::from_fn(|x| {
                    record_key(Dimension::Overworld, Layer::Surface, x as i32, 0)
                });
                for key in [keys[0], keys[1], keys[0], keys[2]] {
                    assert!(b.get(key).is_some());
                }
                assert!(b.reads.contains_key(&keys[0]));
                assert!(!b.reads.contains_key(&keys[1]));
                assert!(b.reads.contains_key(&keys[2]));
                assert_eq!(b.read_order.len(), b.reads.len());
                assert!(b.read_bytes <= b.limit);
            }
            #[test]
            fn gpu_minimap_clips_rotated_uvs_without_stretching() {
                let mut v = view(Dimension::Overworld);
                v.span = [64., 64.];
                for round in [false, true] {
                    v.round = round;
                    for angle in [0., 0.37, std::f64::consts::FRAC_PI_2, std::f64::consts::PI] {
                        v.angle = angle;
                        for bounds in [[-32, -32, 64, 64], [-8, 0, 16, 16], [1000, 1000, 16, 16]] {
                            let points = terrain_polygon(&v, bounds, [16., 32., 16., 16.]);
                            for p in points {
                                assert!(p[0].abs() <= 0.500001 && p[1].abs() <= 0.500001);
                                if round {
                                    assert!(p[0].hypot(p[1]) <= 0.500001);
                                }
                                let world = v.world_at(
                                    (p[0] + 0.5) * v.width as f64,
                                    (p[1] + 0.5) * v.height as f64,
                                );
                                assert!(
                                    (world[0]
                                        - (bounds[0] as f64 + (p[2] - 16.) / 16. * bounds[2] as f64))
                                        .abs()
                                        < 1e-8
                                );
                                assert!(
                                    (world[1]
                                        - (bounds[1] as f64 + (p[3] - 32.) / 16. * bounds[3] as f64))
                                        .abs()
                                        < 1e-8
                                );
                            }
                        }
                    }
                }
            }
            #[test]
            fn reopened_lod_raster_reuses_footprints_instead_of_querying_every_pixel() {
                let dir = CacheDir::new("lod-cursors");
                let key = LodKey {
                    x: 0,
                    z: 0,
                    level: 8,
                };
                {
                    let mut cache = Cache::open(&dir.0, "save:lod-cursors", 1024 * 1024).unwrap();
                    cache.insert_lod(Dimension::Overworld, key, coarse_tile([20, 40, 60]));
                    cache
                        .save(
                            lod_record(Dimension::Overworld, key),
                            cache.lod_tiles[&(Dimension::Overworld, key)].1.clone(),
                        )
                        .unwrap();
                    cache.flush().unwrap();
                }
                let cache = Cache::open(&dir.0, "save:lod-cursors", 1024 * 1024).unwrap();
                let mut v = view(Dimension::Overworld);
                v.center = [128.; 2];
                v.span = [256.; 2];
                v.width = 256;
                v.height = 256;
                let frame = cache.render(v, 1);
                assert!(frame.pixels.chunks_exact(4).all(|p| p == [20, 40, 60, 255]));
                let backing = cache.backing.as_ref().unwrap().borrow();
                assert!(
                    backing.clock < 5000,
                    "{} lookups for one LOD footprint",
                    backing.clock
                );
                assert!(backing.read_bytes <= backing.limit);
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
                for span in [32.0, 256.0, 2048.0] {
                    view.span = [span; 2];
                    let build = Instant::now();
                    let atlas = cache.terrain_atlas(&view, 1);
                    eprintln!(
                        "span={span}: atlas {:?}, {} tiles, {} bytes",
                        build.elapsed(),
                        atlas.chunks.len(),
                        atlas.pixels.len()
                    );
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
                assert!(cache.gpu_atlas_fits(&far));
                let atlas = cache.terrain_atlas(&far, 2);
                assert!(atlas.height > 1024);
                assert!(!atlas.pages.is_empty());
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
    };
}

#[cfg(test)]
pub(crate) use define_tests;
