use std::fmt::Display;

use replay_script_derive::ReplayScript;

trait ReplayScript {
    fn write_replay_script(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result;
}

struct ReplayScriptDisplay<'a, T: ReplayScript>(&'a T);
impl<'a, T: ReplayScript> Display for ReplayScriptDisplay<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.write_replay_script(f)
    }
}

#[derive(ReplayScript)]
struct Config {
    #[script(src_rel = "content1.txt", enable_when = true)]
    foo1: bool,

    #[script(src_rel = "content2.txt", enable = "== 3")]
    foo2: u8,
}

#[test]
fn test_get_config() {
    for (a, b) in itertools::iproduct!([true, false], [true, false]) {
        let cfg = Config {
            foo1: a,
            foo2: if b { 3 } else { 2 },
        };
        let mut expected = String::new();
        if a {
            expected.push_str("stuff1\n\n");
        }
        if b {
            expected.push_str("stuff2\n\n");
        }
        let result = format!("{}", ReplayScriptDisplay(&cfg));
        assert_eq!(result, expected);
    }
}
