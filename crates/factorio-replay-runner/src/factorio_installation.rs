use std::fmt::Display;
use std::path::{Path, PathBuf, absolute};

use crate::utils::{AnyErr, try_download, try_extract};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VersionStr(u8, u8, u16);

impl VersionStr {
    pub fn new(major: u8, minor: u8, patch: u16) -> Self {
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

pub struct FactorioInstallationFolder {
    path: PathBuf,
}

impl FactorioInstallationFolder {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        if !path.exists() || !path.is_dir() {
            Err("Not a directory".to_string())
        } else {
            Ok(FactorioInstallationFolder { path })
        }
    }

    async fn download_factorio(&self, version: VersionStr) -> Result<(), AnyErr> {
        download_factorio(version, &self.path).await
    }
}

async fn download_factorio(
    version: VersionStr,
    out_folder: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!(
        "https://factorio.com/get-download/{}/headless/linux64",
        version
    );
    let zip_path = absolute(&out_folder.join(format!("factorio-{}.tar.xz", version)))?;
    println!(
        "Downloading Factorio version {} to {}",
        version,
        zip_path.display()
    );
    try_download(&url, &zip_path).await?;
    let out_path = absolute(&out_folder.join(version.to_string()))?;
    println!(
        "Extracting {} to {}",
        zip_path.display(),
        out_path.display()
    );
    try_extract(&zip_path, &out_path).await?;
    std::fs::remove_file(zip_path)?;
    Ok(())
}

pub struct FactorioInstallation {
    install_dir: PathBuf,
}

impl FactorioInstallation {
    pub fn install_dir(&self) -> &Path {
        &self.install_dir
    }
}

impl FactorioInstallationFolder {
    pub fn get_factorio(&self, version: VersionStr) -> Option<FactorioInstallation> {
        let path = self.path.join(version.to_string());
        path.exists()
            .then(|| FactorioInstallation { install_dir: path })
    }

    pub async fn get_or_download_factorio(
        &self,
        version: VersionStr,
    ) -> Result<FactorioInstallation, AnyErr> {
        if let Some(installation) = self.get_factorio(version) {
            Ok(installation)
        } else {
            self.download_factorio(version).await?;
            self.get_factorio(version)
                .ok_or_else(|| "Failed to find factorio after download".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_str() {
        let version = VersionStr::try_from("1.2.3").unwrap();
        assert_eq!(version.to_string(), "1.2.3");
        assert_eq!(version, VersionStr(1, 2, 3))
    }

    use std::fs::{self, File, create_dir};
    use tempfile::TempDir;

    #[test]
    fn test_get_versions() -> Result<(), AnyErr> {
        let temp_dir = TempDir::new()?;
        let path = temp_dir.path();

        create_dir(path.join("1.2.3"))?;
        create_dir(path.join("2.3.4"))?;
        create_dir(path.join("ignored"))?;
        File::create(path.join("3.4.5"))?; // also ignored

        let folder = FactorioInstallationFolder::new(path)?;
        assert!(folder.get_factorio(VersionStr(1, 2, 3)).is_some());
        assert!(folder.get_factorio(VersionStr(2, 3, 4)).is_some());
        assert!(!folder.get_factorio(VersionStr(3, 4, 5)).is_none());

        drop(temp_dir);
        Ok(())
    }

    #[tokio::test]
    #[ignore]
    async fn test_download_factorio() -> Result<(), AnyErr> {
        // let temp_dir = TempDir::new()?.keep();
        let temp_dir = PathBuf::from("/tmp/factorio-replay-runner");
        fs::create_dir_all(&temp_dir)?;
        println!("Temp dir: {:?}", temp_dir);
        let version = VersionStr(2, 0, 53);
        download_factorio(version, &temp_dir).await?;
        Ok(())
    }
}
