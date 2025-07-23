use std::path::PathBuf;

/// Get the workspace root directory
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Get the shared test temporary directory
pub fn test_tmp_dir() -> PathBuf {
    workspace_root().join("test_tmp")
}

/// Get the shared fixtures directory
pub fn fixtures_dir() -> PathBuf {
    workspace_root().join("fixtures")
}

/// Get the shared factorio installs directory for tests
pub fn test_factorio_installs_dir() -> PathBuf {
    test_tmp_dir().join("factorio_installs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_root_exists() {
        let root = workspace_root();
        assert!(root.exists(), "Workspace root should exist");
        assert!(root.is_dir(), "Workspace root should be a directory");

        // Should contain Cargo.toml at the workspace root
        let cargo_toml = root.join("Cargo.toml");
        assert!(cargo_toml.exists(), "Workspace Cargo.toml should exist");
    }

    #[test]
    fn test_path_functions() {
        let root = workspace_root();

        assert_eq!(test_tmp_dir(), root.join("test_tmp"));
        assert_eq!(fixtures_dir(), root.join("fixtures"));
        assert_eq!(
            test_factorio_installs_dir(),
            root.join("test_tmp").join("factorio_installs")
        );
    }

    #[test]
    fn test_fixtures_dir_contains_test_zip() {
        let fixtures = fixtures_dir();
        let test_zip = fixtures.join("TEST.zip");
        assert!(
            test_zip.exists(),
            "TEST.zip should exist in fixtures directory"
        );
    }
}
