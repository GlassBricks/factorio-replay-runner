import { add_lib, type EventLib } from "event_handler"
import * as util from "util"

declare global {
  // API
  var ReplayLog: {
    err(...args: string[]): void
    warn(...args: string[]): void
    info(...args: string[]): void
  }
  var addReplayLib: (lib: ReplayLib) => void
  var afterReplay: (fn: () => void) => void
  var PARAM_VALUE: any

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
    _replay_script_DATA: LuaSet<String>
  }
  var util: typeof import("util")
}
_G.util = util

type MsgType = "Error" | "Warn" | "Info"
function logEvent(type: MsgType, ...args: string[]): void {
  print(
    "replay_script_EVENT:",
    game.ticks_played,
    type,
    table.concat(args, " "),
  )
}
ReplayLog = {
  err(...args: string[]): void {
    logEvent("Error", ...args)
  },
  warn(...args: string[]): void {
    logEvent("Warn", ...args)
  },
  info(...args: string[]): void {
    logEvent("Info", ...args)
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
    storage._replay_script_DATA = new LuaSet<String>()
  },
  on_load() {
    if (storage._replay_script_DATA != undefined) return
    script.on_event(defines.events.on_tick, () => {
      for (const fn of afterReplayFns) fn()
    })
  },
})
