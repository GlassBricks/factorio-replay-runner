pub const REPLAY_SCRIPT_CONTROL_LUA: &str = include_str!("../replay-script/control.lua");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replay_script_lua_is_available() {
        let lua_content = REPLAY_SCRIPT_CONTROL_LUA;
        assert!(!lua_content.is_empty());
        assert!(lua_content.contains("TypeScriptToLua"));
        assert!(lua_content.contains("REPLAY_SCRIPT"));
    }
}
