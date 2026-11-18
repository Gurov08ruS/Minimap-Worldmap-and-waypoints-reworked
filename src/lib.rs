// SPDX-License-Identifier: GPL-3.0-only
// Match the sibling test-module layout of the native SDK crate.
#[path = "../mixins/minimap_worldmap_waypoints_reworked/runtime.rs"]
mod minimap_worldmap_waypoints_reworked;

pub use minimap_worldmap_waypoints_reworked::*;

#[cfg(test)]
#[path = "../mixins/minimap_worldmap_waypoints_reworked/container_tests.rs"]
mod minimap_container_tests;
#[cfg(test)]
#[path = "../mixins/minimap_worldmap_waypoints_reworked/map_regression_tests.rs"]
mod minimap_map_regression_tests;
#[cfg(test)]
#[path = "../mixins/minimap_worldmap_waypoints_reworked/maps_tests.rs"]
mod minimap_maps_tests;
#[cfg(test)]
#[path = "../mixins/minimap_worldmap_waypoints_reworked/settings_tests.rs"]
mod minimap_settings_tests;
