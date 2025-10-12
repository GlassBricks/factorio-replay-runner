// default: true
addReplayLib({
  on_nth_tick: {
    [60 * 15]: () => {
      const totalSeconds = Math.floor(game.ticks_played / 60)
      const totalMinutes = Math.floor(totalSeconds / 60)
      const hours = Math.floor(totalMinutes / 60)
      const seconds = totalSeconds % 60
      const minutes = totalMinutes % 60

      let shouldLog = false
      if (hours < 2) {
        shouldLog = minutes % 5 == 0 && seconds == 0 // every 5 mins
      } else if (hours < 4) {
        shouldLog = seconds == 0 // every minute
      } else if (hours < 5) {
        shouldLog = seconds % 30 === 0 // every 30 seconds
      } else {
        shouldLog = seconds % 15 === 0 // every 15 seconds
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
