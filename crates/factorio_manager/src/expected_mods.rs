use crate::error::FactorioError;
use crate::mod_versions::ModVersions;
use std::collections::HashSet;

pub type ExpectedMods = HashSet<String>;

pub fn check_expected_mods(
    expected_mods: &ExpectedMods,
    actual_mods: &ModVersions,
) -> Result<(), FactorioError> {
    let actual_mod_list = actual_mods.keys().cloned().collect::<HashSet<String>>();

    if expected_mods != &actual_mod_list {
        let extra_mods = actual_mod_list
            .difference(expected_mods)
            .map(|s| s.clone())
            .collect::<Vec<String>>();
        let missing_mods = expected_mods
            .difference(&actual_mod_list)
            .map(|s| s.clone())
            .collect::<Vec<String>>();

        return Err(FactorioError::ModMismatch {
            missing_mods,
            extra_mods,
        });
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
        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("quality"));
        assert!(err_msg.contains("space-age"));
    }
}
