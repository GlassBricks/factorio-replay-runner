// default: true
addReplayLib({
  on_nth_tick: {
    [3600 * 1]: () => {
      const totalMinutes = Math.floor(game.ticks_played / 60 / 60)
      const shouldLog =
        totalMinutes <= 120 ? totalMinutes % 5 === 0 : totalMinutes % 1 === 0

      if (shouldLog) {
        const hours = Math.floor(totalMinutes / 60)
        const minutes = totalMinutes % 60
        ReplayLog.info(
          `${hours.toString().padStart(2, "0")}:${minutes.toString().padStart(2, "0")}`,
        )
      }
    },
  },
})
