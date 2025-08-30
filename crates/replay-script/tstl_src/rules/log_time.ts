// default: true
addReplayLib({
  on_nth_tick: {
    [60 * 15]: () => {
      const seconds = Math.floor(game.ticks_played / 60)
      const minutes = Math.floor(seconds / 60)
      const hours = Math.floor(minutes / 60)

      let shouldLog = false
      if (hours < 2) {
        shouldLog = minutes % 5 === 0 && seconds == 0
      } else if (hours < 4) {
        shouldLog = seconds == 0
      } else if (hours < 5) {
        shouldLog = seconds % 30 === 0
      } else if (hours < 6) {
        shouldLog = seconds % 15 === 0
      }

      if (shouldLog) {
        const h = hours
        const m = minutes % 60
        const s = seconds % 60
        ReplayLog.info(string.format("%02d:%02d:%02d", h, m, s))
      }
    },
  },
})
