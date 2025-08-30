// param_type: Option<u16>
// default: Some(1)
addReplayLib({
  on_player_joined_game(event) {
    const maxPlayers: number = PARAM_VALUE
    if (game.players.length() > maxPlayers) {
      ReplayLog.err(
        `Too many players! Maximum allowed: ${maxPlayers}, current: ${game.players.length()}`,
      )
    }
  },
})
