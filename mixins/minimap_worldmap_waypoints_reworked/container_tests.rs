// SPDX-License-Identifier: GPL-3.0-only
// Shared standalone/SDK tests, expanded in the runtime's private scope.

#[cfg(test)]
macro_rules! define_tests {
    () => {
        mod tests {
            #[test]
            fn image_upgrade_offer_survives_commits_until_user_rebuilds() {
                let dir = TestDir::new("upgrade-offer");
                let world = "save:upgrade";
                let path = world_path(&dir.0, world);
                {
                    let mut s = Store::open(&dir.0, world).unwrap();
                    s.put([1, 0, 0, 0, 0, 0], vec![9]).unwrap();
                    s.commit().unwrap();
                }
                let mut bytes = std::fs::read(&path).unwrap();
                for slot in 0..2 {
                    let h = &mut bytes[slot * PAGE..(slot + 1) * PAGE];
                    put32(h, 44, 1);
                    seal(h);
                }
                std::fs::write(&path, bytes).unwrap();
                assert_eq!(
                    inspect(&path).unwrap().compatibility,
                    Compatibility::RebuildImages
                );
                {
                    let mut s = Store::open_existing(&path, world).unwrap();
                    assert!(s.upgrade_pending);
                    s.put([0, 9, 0, 0, 0, 0], vec![1]).unwrap();
                    s.commit().unwrap();
                }
                assert_eq!(
                    inspect(&path).unwrap().compatibility,
                    Compatibility::RebuildImages
                );
                let service = super::super::maps::Service::with_directory(Some(dir.0.clone()));
                service
                    .rebuild_images(world, path.file_name().unwrap().to_str().unwrap())
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap()
                    .unwrap();
                service.shutdown();
                assert_eq!(
                    inspect(&path).unwrap().compatibility,
                    Compatibility::Current
                );
                let mut s = Store::open_existing(&path, world).unwrap();
                assert_eq!(s.read([1, 0, 0, 0, 0, 0]).unwrap().unwrap(), vec![9]);
            }
            #[test]
            fn catalog_is_read_only_recovers_the_second_header_and_ignores_other_files() {
                let dir = TestDir::new("catalog");
                let world = "server:play.example:25565";
                let path = world_path(&dir.0, world);
                {
                    let mut store = Store::open(&dir.0, world).unwrap();
                    store.put([1, 0, 0, 1, 2, 0], vec![7; 32]).unwrap();
                    store.commit().unwrap();
                }
                let mut bytes = std::fs::read(&path).unwrap();
                bytes[0] = 0;
                std::fs::write(&path, &bytes).unwrap();
                std::fs::write(dir.0.join("not-a-cache.png"), b"x").unwrap();
                let info = inspect(&path).unwrap();
                assert_eq!(info.world.as_deref(), Some(world));
                assert_eq!(info.compatibility, Compatibility::Current);
                let list = super::super::cache_manager::scan(&dir.0).unwrap();
                assert_eq!(list.entries.len(), 1);
                assert_eq!(list.entries[0].bytes, bytes.len() as u64);
                assert_eq!(list.entries[0].name, "play.example:25565");
                assert_eq!(std::fs::read(&path).unwrap(), bytes);
            }
            #[test]
            fn future_format_and_visual_revision_are_never_rewritten() {
                for (schema, visual) in [(SCHEMA + 1, 0), (SCHEMA, VISUAL_REVISION + 1)] {
                    let dir = TestDir::new("future-manager");
                    let world = "save:future";
                    let path = world_path(&dir.0, world);
                    {
                        let mut s = Store::open(&dir.0, world).unwrap();
                        s.put([1, 0, 0, 0, 0, 0], vec![1]).unwrap();
                        s.commit().unwrap();
                    }
                    let mut bytes = std::fs::read(&path).unwrap();
                    for slot in 0..2 {
                        let h = &mut bytes[slot * PAGE..(slot + 1) * PAGE];
                        put32(h, 8, schema);
                        put32(h, 44, visual);
                        seal(h);
                    }
                    std::fs::write(&path, &bytes).unwrap();
                    assert!(matches!(
                        inspect(&path).unwrap().compatibility,
                        Compatibility::Newer(_)
                    ));
                    assert!(Store::open_existing(&path, world).is_err());
                    assert!(Store::open(&dir.0, world).is_err());
                    assert_eq!(std::fs::read(&path).unwrap(), bytes);
                }
                assert_eq!(compatibility(SCHEMA, 0), Compatibility::RebuildImages);
                assert!(matches!(
                    compatibility(SCHEMA - 1, 0),
                    Compatibility::Older(_)
                ));
            }
            #[test]
            fn maintenance_never_creates_or_initializes_a_missing_or_empty_cache() {
                let dir = TestDir::new("maintenance-existing");
                let path = world_path(&dir.0, "save:missing");
                assert!(Store::open_existing(&path, "save:missing").is_err());
                assert!(!path.exists());
                let bytes = vec![0; PREFIX as usize];
                std::fs::write(&path, &bytes).unwrap();
                assert!(Store::open_existing(&path, "save:missing").is_err());
                assert_eq!(std::fs::read(&path).unwrap(), bytes);
            }
            #[test]
            fn catalog_resolver_is_async_and_refreshes_names_without_touching_containers() {
                let dir = TestDir::new("catalog-worker");
                let world = "save:folder";
                {
                    let _store = Store::open(&dir.0, world).unwrap();
                }
                let before = std::fs::read(world_path(&dir.0, world)).unwrap();
                let service = super::super::cache_manager::Service::new(dir.0.clone());
                let (entered, ready) = std::sync::mpsc::sync_channel(1);
                let (release, wait) = std::sync::mpsc::sync_channel(1);
                assert!(service.refresh(move |rows| {
                    entered.send(()).unwrap();
                    wait.recv().unwrap();
                    rows[0].name = "Displayed world".into();
                    Ok(())
                }));
                ready
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                assert!(service.snapshot().loading);
                assert!(!service.refresh(|_| Ok(())));
                release.send(()).unwrap();
                let start = std::time::Instant::now();
                while service.snapshot().loading {
                    assert!(start.elapsed() < std::time::Duration::from_secs(5));
                    std::thread::yield_now();
                }
                assert_eq!(service.snapshot().entries[0].name, "Displayed world");
                service.shutdown();
                assert_eq!(std::fs::read(world_path(&dir.0, world)).unwrap(), before);
            }
            #[test]
            fn spatial_queries_match_history_before_and_after_recovery() {
                let dir = TestDir::new("spatial");
                let mut store = Store::open(&dir.0, "save:spatial").unwrap();
                for x in -40..40 {
                    for z in [-33, -1, 0, 31, 32, 1000] {
                        for (d, kind, y, level) in
                            [(1, 0, 0, 0), (1, 1, -16, 0), (2, 0, 0, 0), (1, 2, 0, 3)]
                        {
                            store.put([d, kind, y, x, z, level], vec![1; 8]).unwrap();
                        }
                    }
                }
                let check = |store: &Store| {
                    for (kind, y, level) in [(0, 0, 0), (1, -16, 0), (2, 0, 3)] {
                        let size = 16_i64 << level;
                        let rect = [-35 * size, -size, 70 * size, 34 * size];
                        let expected: std::collections::BTreeSet<_> = store
                            .keys()
                            .filter(|k| {
                                k[0] == 1
                                    && k[1] == kind
                                    && k[2] == y
                                    && (rect[0]..rect[0] + rect[2])
                                        .contains(&(i64::from(k[3]) * size))
                                    && (rect[1]..rect[1] + rect[3])
                                        .contains(&(i64::from(k[4]) * size))
                            })
                            .collect();
                        assert_eq!(
                            store
                                .query(1, kind, y, rect, usize::MAX)
                                .into_iter()
                                .collect::<std::collections::BTreeSet<_>>(),
                            expected
                        );
                        assert_eq!(store.query(1, kind, y, rect, 7).len(), 7);
                    }
                    assert!(store.query(1, 0, 0, [0, 0, 0, 16], 10).is_empty());
                };
                check(&store);
                store.commit().unwrap();
                drop(store);
                let store = Store::open(&dir.0, "save:spatial").unwrap();
                check(&store);
            }

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
                    static NEXT: std::sync::atomic::AtomicU64 =
                        std::sync::atomic::AtomicU64::new(0);
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
            fn cache_files_are_named_after_the_world_and_legacy_files_are_kept() {
                let dir = TestDir::new("names");
                let save = world_path(&dir.0, "save:My World");
                let name = save.file_name().unwrap().to_str().unwrap();
                assert!(name.starts_with("save-My World-"), "{name}");
                assert!(name.ends_with(".mwwr"));
                let server = world_path(&dir.0, "server:play.example.com:25565");
                let name = server.file_name().unwrap().to_str().unwrap();
                assert!(name.starts_with("server-play.example.com_25565-"), "{name}");
                // Worlds whose names sanitize the same still get different files.
                assert_ne!(
                    world_path(&dir.0, "save:a/b"),
                    world_path(&dir.0, "save:a?b")
                );
                // A cache written under the old hash-only name is renamed on open.
                let world = "server:legacy:25565";
                let (a, b) = cache_hash_pair(world);
                let legacy = dir.0.join(format!("{a:016x}{b:016x}.mwwr"));
                drop(Store::open(&dir.0, world).unwrap());
                std::fs::rename(world_path(&dir.0, world), &legacy).unwrap();
                assert!(!world_path(&dir.0, world).exists());
                drop(Store::open(&dir.0, world).unwrap());
                assert!(!legacy.exists());
                assert!(world_path(&dir.0, world).exists());
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
    };
}

#[cfg(test)]
pub(crate) use define_tests;
