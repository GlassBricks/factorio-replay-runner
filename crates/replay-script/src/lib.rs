use serde::{Deserialize, Serialize};
use std::{fmt::Debug, str::FromStr};
use strum::{Display, EnumString, VariantArray};

macro_rules! generate_replay_scripts {
    ($($file_name:ident),*) => {
        #[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
        pub struct ReplayScripts {
            $(#[serde(default)]
            pub $file_name: bool,)*
        }

        impl ReplayScripts {
            pub fn all_enabled() -> Self {
                Self {
                    $($file_name: true,)*
                }
            }
        }

        impl std::fmt::Display for ReplayScripts {
            fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
                fmt.write_str(include_str!(concat!(env!("OUT_DIR"), "/main.lua")))?;
                $(
                    if self.$file_name {
                        writeln!(fmt, "-- Script for {}", stringify!($file_name))?;
                        fmt.write_str(include_str!(concat!(env!("OUT_DIR"), "/rules/", stringify!($file_name), ".lua")))?;
                    }
                )*
                Ok(())
            }
        }
    };
}
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
    fn test_write_replay_scripts() {
        let replay_scripts = ReplayScripts {
            check_console_commands: true,
            ..Default::default()
        };

        let output = replay_scripts.to_string();
        let expected = include_str!(concat!(env!("OUT_DIR"), "/main.lua")).to_string()
            + "-- Script for check_console_commands\n"
            + include_str!(concat!(
                env!("OUT_DIR"),
                "/rules/check_console_commands.lua"
            ));
        assert_eq!(output, expected);
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

    #[test]
    fn test_all_enabled() {
        let all_enabled = ReplayScripts::all_enabled();
        assert!(all_enabled.check_console_commands);
        assert!(all_enabled.first_rocket_launched);
        assert!(all_enabled.no_blueprint_import);
        assert!(all_enabled.no_map_editor);
        assert!(all_enabled.open_other_player);
    }

    #[test]
    fn test_no_export_in_replay_script() {
        let replay_scripts = ReplayScripts::all_enabled();
        let output = replay_scripts.to_string();
        println!("{}", output);
        println!("{}", output.contains("____exports"));
        for pattern in ["return ____exports"] {
            assert!(!output.contains(pattern));
        }
    }
}
