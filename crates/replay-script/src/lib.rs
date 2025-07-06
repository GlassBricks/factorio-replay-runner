use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use strum::{Display, EnumString, VariantArray};

macro_rules! generate_replay_scripts {
    ($($file_name:ident),*) => {
        #[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
        pub struct ReplayScripts {
            $(pub $file_name: bool,)*
        }

        impl std::fmt::Display for ReplayScripts {
            fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
                $(
                    if self.$file_name {
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
    pub msg_type: MsgType,
    pub time: u64,
    pub message: String,
}

pub const REPLAY_SCRIPT_EVENT_PREFIX: &str = "REPLAY_SCRIPT_EVENT:";

impl TryFrom<&str> for ReplayMsg {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, ()> {
        let parts: Vec<&str> = value.split('\t').collect();
        if parts.len() != 4 || parts[0] != REPLAY_SCRIPT_EVENT_PREFIX {
            return Err(());
        };
        Ok(ReplayMsg {
            msg_type: MsgType::try_from(parts[1]).map_err(|_| ())?,
            time: parts[2].parse().map_err(|_| ())?,
            message: parts[3].to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use itertools::iproduct;

    use super::*;

    #[test]
    fn test_write_replay_scripts() {
        let replay_scripts = ReplayScripts {
            check_console_commands: true,
            ..Default::default()
        };

        let output = replay_scripts.to_string();
        let expected = include_str!(concat!(
            env!("OUT_DIR"),
            "/rules/check_console_commands.lua"
        ));
        assert_eq!(output, expected);
    }

    #[test]
    fn test_parse_msg() {
        for (&msg_type, time, msg) in
            iproduct!(MsgType::VARIANTS, [1234, 2345], ["message1", "message2"])
        {
            let formatted = ReplayMsg::try_from(
                format!(
                    "{REPLAY_SCRIPT_EVENT_PREFIX}\t{}\t{}\t{}",
                    msg_type, time, msg
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
