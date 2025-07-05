import { add_lib, type EventLib } from "event_handler"

declare global {
  // API
  var ReplayLog: {
    err(...args: string[]): void
    warn(...args: string[]): void
    info(...args: string[]): void
  }
  var addReplayLib: (lib: ReplayLib) => void
  var afterReplay: (fn: () => void) => void

  type NamedEvents = {
    [K in keyof typeof defines.events]?: (
      this: void,
      event: (typeof defines.events)[K]["_eventData"],
    ) => void
  }

  interface ReplayLib extends EventLib, NamedEvents {
    afterReplay?(): void
  }

  // Declares
  const storage: {
    _REPLAY_SCRIPT_DATA: LuaSet<String>
  }
}

type MsgType = "error" | "warn" | "info"
function logEvent(type: MsgType, ...args: string[]): void {
  print(
    "REPLAY_SCRIPT_EVENT:",
    game.ticks_played,
    type,
    table.concat(args, " "),
  )
}
ReplayLog = {
  err(...args: string[]): void {
    logEvent("error", ...args)
  },
  warn(...args: string[]): void {
    logEvent("warn", ...args)
  },
  info(...args: string[]): void {
    logEvent("info", ...args)
  },
}

addReplayLib = (lib: ReplayLib) => {
  if (!lib.events) lib.events = {}
  for (const [name, fn] of pairs(lib)) {
    if (name in defines.events) {
      lib.events[defines.events[name as keyof typeof defines.events]] =
        fn as never
    }
  }
  add_lib(lib)
}
const afterReplayFns: Array<() => void> = []
afterReplay = function (fn: () => void): void {
  afterReplayFns.push(fn)
}

addReplayLib({
  on_init() {
    storage._REPLAY_SCRIPT_DATA = new LuaSet<String>()
  },
  on_load() {
    script.on_event(defines.events.on_tick, () => {
      if (storage._REPLAY_SCRIPT_DATA != undefined) return
      for (const fn of afterReplayFns) fn()
    })
  },
})
