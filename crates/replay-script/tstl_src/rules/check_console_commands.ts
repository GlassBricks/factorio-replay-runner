import * as util from "util"
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

addReplayLib({
  on_console_command(event) {
    const player =
      (event.player_index != undefined &&
        game.get_player(event.player_index)?.name) ||
      "server"

    if (event.command === "editor") {
      // Editor command is handled elsewhere
      return
    }

    if (
      event.command in allowedCommands ||
      (event.command === "config" &&
        event.parameters.trim() === "set allow-debug-settings false")
    ) {
      ReplayLog.info(player, "ran:", `/${event.command}`, event.parameters)
    } else if (event.command === "admin") {
      ReplayLog.warn(player, "ran:", `/${event.command}`, event.parameters)
    } else {
      ReplayLog.err(
        player,
        "ran disallowed command:",
        `/${event.command}`,
        event.parameters,
      )
    }
  },
})
