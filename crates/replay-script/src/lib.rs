use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
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
}
