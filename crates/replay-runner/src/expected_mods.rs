use crate::mod_versions::ModVersions;
use anyhow::Error;
use std::collections::HashSet;

pub type ExpectedMods = HashSet<String>;

pub fn check_expected_mods(
    expected_mods: &ExpectedMods,
    actual_mods: &ModVersions,
) -> anyhow::Result<()> {
    let actual_mod_list = actual_mods.keys().cloned().collect::<HashSet<String>>();

    if expected_mods != &actual_mod_list {
        let extra_mods = actual_mod_list
            .difference(expected_mods)
            .collect::<Vec<_>>();
        let missing_mods = expected_mods
            .difference(&actual_mod_list)
            .collect::<Vec<_>>();
        let msg = format!(
            "Missing mods: {:?}, Extra mods: {:?}",
            missing_mods, extra_mods
        );
        return Err(Error::msg(msg));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::expected_mods::{ExpectedMods, check_expected_mods};
    use std::collections::HashMap;

    #[test]
    fn test_check_expected_mods_match() {
        let expected = ExpectedMods::from(["base".to_string(), "quality".to_string()]);
        let actual = HashMap::from([("base".to_string(), None), ("quality".to_string(), None)]);

        assert!(check_expected_mods(&expected, &actual).is_ok());
    }

    #[test]
    fn test_check_expected_mods_mismatch() {
        let expected = ExpectedMods::from(["base".to_string(), "quality".to_string()]);
        let actual = HashMap::from([("base".to_string(), None), ("space-age".to_string(), None)]);

        let result = check_expected_mods(&expected, &actual);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Missing mods: [\"quality\"], Extra mods: [\"space-age\"]"
        );
    }
}
