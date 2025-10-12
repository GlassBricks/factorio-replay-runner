use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Debug},
    str::FromStr,
};
use strum::{Display, EnumString, VariantArray};

include!(concat!(env!("OUT_DIR"), "/replay_scripts.rs"));

#[derive(Debug, PartialEq, Eq, Copy, Clone, VariantArray, Display, EnumString, PartialOrd, Ord)]
pub enum MsgLevel {
    Info,
    Warn,
    Error,
}

pub struct ReplayMsg {
    pub time: u64,
    pub level: MsgLevel,
    pub message: String,
}

pub const REPLAY_SCRIPT_EVENT_PREFIX: &str = "REPLAY_SCRIPT_EVENT:";
pub const REPLAY_EXIT_SUCCESS_PREFIX: &str = "REPLAY_EXIT_SUCCESS:";

pub struct ExitSignal {
    pub time: u64,
    pub message: String,
}

impl FromStr for ExitSignal {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, ()> {
        let parts: Vec<&str> = value.split('\t').collect();
        if parts.len() != 3 || parts[0] != REPLAY_EXIT_SUCCESS_PREFIX {
            return Err(());
        };
        Ok(ExitSignal {
            time: parts[1].parse().map_err(|_| ())?,
            message: parts[2].to_string(),
        })
    }
}

impl fmt::Display for ExitSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Replay exited successfully at tick {}: {}",
            self.time, self.message
        )
    }
}

impl FromStr for ReplayMsg {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, ()> {
        let parts: Vec<&str> = value.split('\t').collect();
        if parts.len() != 4 || parts[0] != REPLAY_SCRIPT_EVENT_PREFIX {
            return Err(());
        };
        Ok(ReplayMsg {
            time: parts[1].parse().map_err(|_| ())?,
            level: MsgLevel::try_from(parts[2]).map_err(|_| ())?,
            message: parts[3].to_string(),
        })
    }
}

// is NOT the inverse of from_str
impl fmt::Display for ReplayMsg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:5}]\t{:10}\t{}", self.level, self.time, self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use itertools::iproduct;

    #[test]
    fn test_defaults() {
        let output = ReplayScripts::default().to_string();

        let header = include_str!(concat!(env!("OUT_DIR"), "/main.lua"));
        assert!(output.starts_with(header));

        assert!(output.contains("max_players"));
        assert!(output.contains("no_bad_console_commands"));
        assert!(output.contains("no_blueprint_import"));
        assert!(output.contains("no_map_editor"));
        assert!(output.contains("no_open_other_player"));

        assert!(!output.contains("win_on_scenario_finished"));
        assert!(output.contains("local maxPlayers = 1\n"));
    }

    #[test]
    fn test_configure_script() {
        let scripts = ReplayScripts {
            blueprint_import: true,
            win_on_scenario_finished: true,
            max_players: Some(137),
            ..Default::default()
        };

        let output = scripts.to_string();
        assert!(!output.contains("blueprint_import"));
        assert!(output.contains("win_on_scenario_finished"));
        assert!(output.contains("local maxPlayers = 137"));
    }

    #[test]
    fn test_all_enabled() {
        let output = ReplayScripts::all_enabled().to_string();
        for file_name in ReplayScripts::all_scripts() {
            assert!(output.contains(file_name));
        }
    }

    #[test]
    fn test_no_export_in_replay_script() {
        let output = ReplayScripts::default().to_string();
        let pattern = "return ____exports";
        assert!(!output.contains(pattern));
    }

    #[test]
    fn test_serde_defaults() {
        let scripts: ReplayScripts = serde_yaml::from_str("{}").unwrap();

        assert_eq!(scripts.max_players, Some(1));
        assert!(!scripts.bad_console_commands);
        assert!(!scripts.blueprint_import);
        assert!(!scripts.map_editor);
        assert!(!scripts.open_other_player);
        assert!(!scripts.win_on_scenario_finished);

        // Test partial deserialization preserves defaults for missing fields
        let scripts: ReplayScripts = serde_yaml::from_str("win_on_scenario_finished: true").unwrap();
        assert!(scripts.win_on_scenario_finished);
        assert_eq!(scripts.max_players, Some(1)); // Should still use configured default
    }

    #[test]
    fn test_parse_msg() {
        let msg = "REPLAY_SCRIPT_EVENT:\t123\tError\tSome message";
        let msg = ReplayMsg::from_str(msg);
        assert!(msg.is_ok());
        let msg = msg.unwrap();
        assert_eq!(msg.level, MsgLevel::Error);
        assert_eq!(msg.time, 123);
        assert_eq!(msg.message, "Some message");

        for (&msg_type, time, msg) in
            iproduct!(MsgLevel::VARIANTS, [1234, 2345], ["message1", "message2"])
        {
            let formatted = ReplayMsg::from_str(
                format!(
                    "{REPLAY_SCRIPT_EVENT_PREFIX}\t{}\t{}\t{}",
                    time, msg_type, msg
                )
                .as_str(),
            )
            .unwrap();
            assert_eq!(formatted.level, msg_type);
            assert_eq!(formatted.time, time);
            assert_eq!(formatted.message, msg);
        }
    }

    #[test]
    fn test_parse_exit_signal() {
        let exit = "REPLAY_EXIT_SUCCESS:\t456\tScenario finished";
        let exit = ExitSignal::from_str(exit);
        assert!(exit.is_ok());
        let exit = exit.unwrap();
        assert_eq!(exit.time, 456);
        assert_eq!(exit.message, "Scenario finished");

        let invalid = "REPLAY_SCRIPT_EVENT:\t123\tInfo\tNot an exit";
        assert!(ExitSignal::from_str(invalid).is_err());

        let invalid_format = "REPLAY_EXIT_SUCCESS:\tinvalid\tMessage";
        assert!(ExitSignal::from_str(invalid_format).is_err());
    }
}
