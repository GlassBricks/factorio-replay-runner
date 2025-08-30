addReplayLib({
  on_player_toggled_map_editor(event) {
    const player = game.get_player(event.player_index)!.name
    ReplayLog.err(player, "used map editor!")
  },
})
