use anyhow::{Context, Result};
use std::fmt::Display;
use std::path::{Path, PathBuf, absolute};

use crate::factorio_instance::FactorioInstance;
use crate::utils::{try_download, try_extract};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VersionStr(pub u16, pub u16, pub u16);

impl VersionStr {
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        VersionStr(major, minor, patch)
    }
}
impl TryFrom<&str> for VersionStr {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let parts: Vec<&str> = value.split('.').collect();
        if parts.len() != 3 {
            Err("Invalid version format".to_string())
        } else {
            Ok(VersionStr(
                parts[0].parse().map_err(|_| "Invalid major version")?,
                parts[1].parse().map_err(|_| "Invalid minor version")?,
                parts[2].parse().map_err(|_| "Invalid patch version")?,
            ))
        }
    }
}
impl TryFrom<String> for VersionStr {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        VersionStr::try_from(value.as_str())
    }
}
impl Display for VersionStr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

pub struct FactorioInstallDir {
    path: PathBuf,
}

impl FactorioInstallDir {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let path: PathBuf = path.into();
        let path = path
            .canonicalize()
            .with_context(|| format!("Failed to canonicalize path: {}", path.display()))?;
        if !path.exists() || !path.is_dir() {
            anyhow::bail!("Path is not a directory: {}", path.display());
        }
        Ok(FactorioInstallDir { path })
    }

    pub fn new_or_create(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            std::fs::create_dir_all(path)
                .with_context(|| format!("Failed to create directory: {}", path.display()))?;
        }
        let path = path
            .canonicalize()
            .with_context(|| format!("Failed to canonicalize path: {}", path.display()))?;
        Self::new(path)
    }

    async fn download_factorio(&self, version: VersionStr) -> Result<()> {
        download_factorio(version, &self.path).await
    }
}

async fn download_factorio(version: VersionStr, out_folder: &Path) -> Result<()> {
    let url = format!(
        "https://factorio.com/get-download/{}/headless/linux64",
        version
    );
    let zip_path = absolute(&out_folder.join(format!("factorio-{}.tar.xz", version)))?;
    println!("Downloading Factorio {} to {}", version, zip_path.display());
    try_download(&url, &zip_path).await?;
    let out_path = absolute(&out_folder.join(version.to_string()))?;
    println!(
        "Extracting {} to {}",
        zip_path.display(),
        out_path.display()
    );
    try_extract(&zip_path, &out_path).await?;
    let _ = std::fs::remove_file(&zip_path);
    Ok(())
}

impl FactorioInstallDir {
    pub fn get_factorio(&self, version: VersionStr) -> Option<FactorioInstance> {
        let path = self.path.join(version.to_string()).join("factorio");
        path.exists().then(|| FactorioInstance::new_canonical(path))
    }

    pub async fn get_or_download_factorio(&self, version: VersionStr) -> Result<FactorioInstance> {
        if let Some(installation) = self.get_factorio(version) {
            Ok(installation)
        } else {
            self.download_factorio(version).await?;
            self.get_factorio(version)
                .ok_or_else(|| anyhow::anyhow!("Failed to find factorio after download"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl FactorioInstallDir {
        pub(crate) fn test_dir() -> Self {
            Self::new_or_create("./testtmp").expect("Failed to create test directory")
        }
    }

    #[test]
    fn test_version_str() {
        let version = VersionStr::try_from("1.2.3").unwrap();
        assert_eq!(version.to_string(), "1.2.3");
        assert_eq!(version, VersionStr(1, 2, 3))
    }

    use std::fs::{self, File, create_dir, create_dir_all};
    use tempfile::TempDir;

    #[test]
    fn test_get_versions() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let path = temp_dir.path();

        let make_installation = |name: &str| create_dir_all(&path.join(name).join("factorio"));

        make_installation("1.2.3")?;
        make_installation("2.3.4")?;
        create_dir(path.join("ignored"))?;
        create_dir(path.join("3.4.5"))?; // ignored, since no nested factorio folder
        File::create(path.join("4.5.6"))?; // also ignored

        let folder = FactorioInstallDir::new(path)?;
        assert!(folder.get_factorio(VersionStr(1, 2, 3)).is_some());
        assert!(folder.get_factorio(VersionStr(2, 3, 4)).is_some());
        assert!(folder.get_factorio(VersionStr(3, 4, 5)).is_none());
        assert!(folder.get_factorio(VersionStr(4, 5, 6)).is_none());

        drop(temp_dir);
        Ok(())
    }

    #[async_std::test]
    #[ignore]
    async fn test_download_factorio() -> Result<()> {
        // let temp_dir = TempDir::new()?.keep();
        let temp_dir = PathBuf::from("/tmp/factorio-replay-runner");
        fs::create_dir_all(&temp_dir)?;
        println!("Temp dir: {:?}", temp_dir);
        let version = VersionStr(2, 0, 53);
        download_factorio(version, &temp_dir).await?;
        Ok(())
    }
}
