// SPDX-License-Identifier: GPL-3.0-only
fn client_packet(
    game: &mut crate::game::Game<'_>,
    packet: &horizon_client::mod_api::ClientboundPlayPacket,
) {
    crate::game::reworked_maps::client_packet(game, packet);
}
