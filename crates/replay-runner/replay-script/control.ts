import { add_lib, Events } from "event_handler"
import {
  OnGuiOpenedEvent,
  OnPlayerCursorStackChangedEvent,
} from "factorio:runtime"
import * as util from "util"

declare const storage: {
  replayScriptOneTimeEvents: LuaSet<string>
}
const allowedCommands = util.list_to_map([
  "admins",
  "ban",
  "banlist",
  "bans",
  "clear",
  "color",
  "demote",
  "evolution",
  "h",
  "help",
  "ignore",
  "ignores",
  "kick",
  "mute",
  "mute-programmable-speaker",
  "mutes",
  "p",
  "players",
  "promote",
  "purge",
  "r",
  "reply",
  "reset-tips",
  "s",
  "screenshot",
  "seed",
  "server-save",
  "shout",
  "time",
  "unban",
  "unignore",
  "unlock-shortcut-bar",
  "unlock-tips",
  "unmute",
  "version",
  "w",
  "whisper",
  "whitelist",
])

const events: {
  [K in keyof typeof defines.events]?: (
    this: void,
    event: (typeof defines.events)[K]["_eventData"],
  ) => void
} = {
  on_console_command(event) {
    checkScriptStartedCorrectly()
    const player =
      (event.player_index != undefined &&
        game.get_player(event.player_index)?.name) ||
      "server"
    if (event.command == "editor") {
      // ignore, handled below
      return
    }
    if (
      event.command in allowedCommands ||
      (event.command == "config" &&
        event.parameters.trim() == "set allow-debug-settings false")
    ) {
      logEvent(`ℹ️ ${player} ran: /${event.command} ${event.parameters}`)
    } else if (event.command == "admin") {
      logEvent(`⚠️ ${player} ran: /${event.command} ${event.parameters}`)
    } else {
      logEvent(
        `🚫 ${player} ran disallowed command: /${event.command} ${event.parameters}`,
      )
    }
  },

  on_player_toggled_map_editor(event) {
    const player = game.get_player(event.player_index)!.name
    logEvent(`🚫 ${player} used map editor!`)
  },

  on_rocket_launched() {
    logEventOnce("ℹ️: First rocket launched")
  },

  on_gui_opened(event: OnGuiOpenedEvent) {
    checkScriptStartedCorrectly()
    if (event.gui_type == defines.gui_type.other_player) {
      const player = game.get_player(event.player_index)
      const otherPlayer = event.other_player
      logEvent(`⚠️ ${player?.name} opened ${otherPlayer?.name}'s GUI!`)
    }
  },

  on_player_cursor_stack_changed(event: OnPlayerCursorStackChangedEvent) {
    checkScriptStartedCorrectly()
    if ("import-blueprint" in storage.replayScriptOneTimeEvents) return
    const player = game.get_player(event.player_index)!
    const record = player.cursor_record
    if (record && !record.valid_for_write) {
      logEventOnce(
        "import-blueprint",
        `⚠️ ${player.name} imported a blueprint from the blueprint library!`,
      )
    }
  },

  on_tick: checkScriptStartedCorrectly,
  on_player_changed_position: checkScriptStartedCorrectly,
  on_selected_entity_changed: checkScriptStartedCorrectly,
}

function logEvent(msg: string) {
  print(string.format("REPLAY_SCRIPT:%8.2f %s", game.ticks_played / 60, msg))
}

function logEventOnce(key: string, msg: string = key) {
  if (!storage.replayScriptOneTimeEvents.has(key)) {
    storage.replayScriptOneTimeEvents.add(key)
    logEvent(msg)
  }
}

function checkScriptStartedCorrectly() {
  if (storage.replayScriptOneTimeEvents !== undefined) return
  game.tick_paused = false
  game.set_win_ending_info({
    title: "Replay Script: incorrectly started",
    message:
      "Did you mean to start the replay instead of loading the save file?\n" +
      "VVVV click Finish to return to main menu",
  })
  game.set_game_state({
    game_finished: true,
    player_won: true,
    can_continue: false,
  })
}

const eventsTable: Events = {}
for (const [name, fn] of pairs(events)) {
  eventsTable[defines.events[name]] = fn as any
}

add_lib({
  events: eventsTable,
  on_init() {
    storage.replayScriptOneTimeEvents = new LuaSet()
    logEvent("Started replay script!")
  },
})
