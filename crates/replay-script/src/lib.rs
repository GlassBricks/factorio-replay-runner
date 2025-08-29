use serde::{Deserialize, Serialize};
use std::{fmt::Debug, str::FromStr};
use strum::{Display, EnumString, VariantArray};

include!(concat!(env!("OUT_DIR"), "/replay_scripts.rs"));

#[derive(Debug, PartialEq, Eq, Copy, Clone, VariantArray, Display, EnumString)]
pub enum MsgType {
    Info,
    Warn,
    Error,
}

pub struct ReplayMsg {
    pub time: u64,
    pub msg_type: MsgType,
    pub message: String,
}

pub const REPLAY_SCRIPT_EVENT_PREFIX: &str = "REPLAY_SCRIPT_EVENT:";

impl FromStr for ReplayMsg {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, ()> {
        let parts: Vec<&str> = value.split('\t').collect();
        if parts.len() != 4 || parts[0] != REPLAY_SCRIPT_EVENT_PREFIX {
            return Err(());
        };
        Ok(ReplayMsg {
            time: parts[1].parse().map_err(|_| ())?,
            msg_type: MsgType::try_from(parts[2]).map_err(|_| ())?,
            message: parts[3].to_string(),
        })
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

        assert!(!output.contains("win_on_rocket_launch"));
        assert!(output.contains("local maxPlayers = 1\n"));
    }

    #[test]
    fn test_configure_script() {
        let mut scripts = ReplayScripts::default();
        scripts.blueprint_import = true;
        scripts.win_on_rocket_launch = true;
        scripts.max_players = Some(137);

        let output = scripts.to_string();
        assert!(!output.contains("blueprint_import"));
        assert!(output.contains("win_on_rocket_launch"));
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
        for pattern in ["return ____exports"] {
            assert!(!output.contains(pattern));
        }
    }

    #[test]
    fn test_parse_msg() {
        let msg = "REPLAY_SCRIPT_EVENT:\t123\tError\tSome message";
        let msg = ReplayMsg::from_str(msg);
        assert!(msg.is_ok());
        let msg = msg.unwrap();
        assert_eq!(msg.msg_type, MsgType::Error);
        assert_eq!(msg.time, 123);
        assert_eq!(msg.message, "Some message");

        for (&msg_type, time, msg) in
            iproduct!(MsgType::VARIANTS, [1234, 2345], ["message1", "message2"])
        {
            let formatted = ReplayMsg::from_str(
                format!(
                    "{REPLAY_SCRIPT_EVENT_PREFIX}\t{}\t{}\t{}",
                    time, msg_type, msg
                )
                .as_str(),
            )
            .unwrap();
            assert_eq!(formatted.msg_type, msg_type);
            assert_eq!(formatted.time, time);
            assert_eq!(formatted.message, msg);
        }
    }
}
