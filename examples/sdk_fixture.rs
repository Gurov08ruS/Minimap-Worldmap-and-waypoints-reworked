// SPDX-License-Identifier: GPL-3.0-only
//! Two Cyrillic markers for the disposable SDK-test world; never a player directory.
use minimap_worldmap_waypoints_reworked as rt;
use std::{fs, path::PathBuf};
fn main() {
    let directory = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("mod data directory inside target/"),
    )
    .canonicalize()
    .expect("run the isolated SDK instance once first");
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .canonicalize()
        .unwrap();
    assert!(
        directory.starts_with(target),
        "fixture must stay inside target/"
    );
    let controller = rt::Controller::open(directory.clone());
    for (x, name, color) in [(92, "База", 0x5aa9ff), (96, "Бункер", 0x5eaaff)] {
        controller.add_waypoint(rt::Waypoint {
            world: "save:SDK-test".into(),
            dimension: rt::Dimension::Overworld,
            x,
            y: 131,
            z: -10,
            name: name.into(),
            color,
            icon: rt::WaypointIcon::Letter,
            biome: None,
            show_3d: true,
        });
    }
    for button in [0, 13, 20] {
        assert!(controller.set_option(button, 0));
    }
    let style = rt::minimap::Settings {
        world_waypoint_style: rt::minimap::WorldMarkerStyle::LetterDistance,
        ..Default::default()
    };
    assert!(controller.set_option(14, style.option_index(14).unwrap()));
    controller.shutdown();
    let settings = controller.settings();
    for (key, button) in rt::SDK_SETTING_ROWS {
        fs::write(
            directory.join(key),
            settings.option_index(button).unwrap().to_string(),
        )
        .unwrap();
    }
    println!("Prepared База/Бункер in the isolated SDK-test world");
}
