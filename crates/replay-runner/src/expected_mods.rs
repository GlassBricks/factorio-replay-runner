use crate::mod_versions::ModVersions;
use anyhow::Error;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum ExpectedMods {
    Base,
    SpaceAge,
}

pub fn check_expected_mods(
    expected_mods: &ExpectedMods,
    actual_mods: &ModVersions,
) -> anyhow::Result<()> {
    let expected_mods = match expected_mods {
        ExpectedMods::Base => HashSet::from(["base"]),
        ExpectedMods::SpaceAge => HashSet::from(["base", "space-age", "quality", "elevated-rails"]),
    };

    let actual_mod_list = actual_mods
        .keys()
        .map(String::as_str)
        .collect::<HashSet<&str>>();

    if expected_mods != actual_mod_list {
        let extra_mods = actual_mod_list
            .difference(&expected_mods)
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
    use crate::factorio_install_dir::VersionStr;
    use crate::mod_versions::ModVersions;
    use std::collections::HashMap;

    fn create_base_only_mods() -> ModVersions {
        let mut mods = HashMap::new();
        mods.insert("base".to_string(), None);
        mods
    }

    fn create_space_age_mods() -> ModVersions {
        let mut mods = HashMap::new();
        mods.insert(
            "base".to_string(),
            VersionStr::try_from("2.0.15".to_string()).ok(),
        );
        mods.insert("space-age".to_string(), None);
        mods.insert("quality".to_string(), None);
        mods.insert("elevated-rails".to_string(), None);
        mods
    }

    #[test]
    fn test_check_expected_mods_base_only_valid() {
        let expected = ExpectedMods::Base;
        let actual = create_base_only_mods();

        assert!(check_expected_mods(&expected, &actual).is_ok());
    }

    #[test]
    fn test_check_space_age_not_allowed() {
        let expected = ExpectedMods::Base;
        let actual = create_space_age_mods();

        assert!(check_expected_mods(&expected, &actual).is_err());
    }

    #[test]
    fn test_check_expected_extra_mods() {
        let expected = ExpectedMods::SpaceAge;
        let mut actual = create_base_only_mods();
        actual.insert(
            "some-extra-mod".to_string(),
            VersionStr::try_from("1.0.0".to_string()).ok(),
        );

        assert!(check_expected_mods(&expected, &actual).is_err());
    }
}
