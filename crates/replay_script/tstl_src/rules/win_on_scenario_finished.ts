// default: false
addReplayLib({
  on_tick() {
    if (game.finished) {
      exitReplay("Scenario finished")
    }
  },
})
