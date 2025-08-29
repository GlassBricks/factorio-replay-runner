addReplayLib({
  on_gui_opened(event) {
    if (event.gui_type === defines.gui_type.other_player) {
      const player = game.get_player(event.player_index)
      const otherPlayer = event.other_player
      ReplayLog.warn(
        player?.name || "unknown",
        "opened",
        otherPlayer?.name || "unknown",
        "player's GUI!",
      )
    }
  },
})
