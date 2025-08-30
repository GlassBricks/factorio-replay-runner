addReplayLib({
  on_player_cursor_stack_changed(event) {
    if ("import-blueprint" in storage._replay_script_DATA) return
    const player = game.get_player(event.player_index)!
    const record = player.cursor_record
    if (record && !record.valid_for_write) {
      storage._replay_script_DATA.add("import-blueprint")
      ReplayLog.err(
        player.name,
        "imported a blueprint from the blueprint library!",
      )
    }
  },
})
