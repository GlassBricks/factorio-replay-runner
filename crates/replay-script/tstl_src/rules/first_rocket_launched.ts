addReplayLib({
  on_rocket_launched() {
    if ("first-rocket" in storage._REPLAY_SCRIPT_DATA) return
    storage._REPLAY_SCRIPT_DATA.add("first-rocket")
    ReplayLog.info("First rocket launched")
  },
})
