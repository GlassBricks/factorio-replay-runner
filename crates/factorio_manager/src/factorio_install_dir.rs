use std::fmt::Display;
use std::path::{Path, PathBuf, absolute};

use crate::cmd::{try_download, try_extract};
use crate::error::FactorioError;
use crate::factorio_instance::FactorioInstance;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VersionStr(pub u16, pub u16, pub u16);

impl VersionStr {
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        VersionStr(major, minor, patch)
    }
}
impl TryFrom<&str> for VersionStr {
    type Error = FactorioError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let parts: Vec<&str> = value.split('.').collect();
        if parts.len() != 3 {
            return Err(FactorioError::InvalidVersion(anyhow::anyhow!(
                "Invalid version format: expected 3 parts, got {}",
                parts.len()
            )));
        }
        let parse_part = |part: &str, name: &str| -> Result<u16, FactorioError> {
            part.parse().map_err(|e| {
                FactorioError::InvalidVersion(
                    anyhow::Error::from(e).context(format!("Invalid {} version", name)),
                )
            })
        };

        Ok(VersionStr(
            parse_part(parts[0], "major")?,
            parse_part(parts[1], "minor")?,
            parse_part(parts[2], "patch")?,
        ))
    }
}
impl TryFrom<String> for VersionStr {
    type Error = FactorioError;

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
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, FactorioError> {
        let path: PathBuf = path.into();
        let path = path.canonicalize().map_err(|e| {
            FactorioError::InstallDirError(
                anyhow::Error::from(e)
                    .context(format!("Failed to canonicalize path: {}", path.display())),
            )
        })?;
        if !path.exists() || !path.is_dir() {
            return Err(FactorioError::InstallDirError(anyhow::anyhow!(
                "Path is not a directory: {}",
                path.display()
            )));
        }
        Ok(FactorioInstallDir { path })
    }

    pub fn new_or_create(path: impl AsRef<Path>) -> Result<Self, FactorioError> {
        let path = path.as_ref();
        if !path.exists() {
            std::fs::create_dir_all(path).map_err(|e| {
                FactorioError::InstallDirError(
                    anyhow::Error::from(e)
                        .context(format!("Failed to create directory: {}", path.display())),
                )
            })?;
        }
        let path = path.canonicalize().map_err(|e| {
            FactorioError::InstallDirError(
                anyhow::Error::from(e)
                    .context(format!("Failed to canonicalize path: {}", path.display())),
            )
        })?;
        Self::new(path)
    }

    async fn download_factorio(&self, version: VersionStr) -> Result<(), FactorioError> {
        download_factorio(version, &self.path).await
    }
}

async fn download_factorio(version: VersionStr, out_folder: &Path) -> Result<(), FactorioError> {
    let url = format!(
        "https://factorio.com/get-download/{}/headless/linux64",
        version
    );
    let zip_path =
        absolute(out_folder.join(format!("factorio-{}.tar.xz", version))).map_err(|e| {
            FactorioError::FactorioDownloadFailed {
                version,
                source: anyhow::Error::from(e),
            }
        })?;
    println!("Downloading Factorio {} to {}", version, zip_path.display());
    try_download(&url, &zip_path)
        .await
        .map_err(|e| FactorioError::FactorioDownloadFailed { version, source: e })?;
    let out_path = absolute(out_folder.join(version.to_string())).map_err(|e| {
        FactorioError::ExtractionFailed(
            anyhow::Error::from(e).context("Failed to get extraction path"),
        )
    })?;
    println!(
        "Extracting {} to {}",
        zip_path.display(),
        out_path.display()
    );
    try_extract(&zip_path, &out_path)
        .await
        .map_err(FactorioError::ExtractionFailed)?;
    let _ = std::fs::remove_file(&zip_path);
    Ok(())
}

impl FactorioInstallDir {
    pub fn get_factorio(&self, version: VersionStr) -> Option<FactorioInstance> {
        let path = self.path.join(version.to_string()).join("factorio");
        path.exists().then(|| FactorioInstance::new(path).unwrap())
    }

    pub async fn get_or_download_factorio(
        &self,
        version: VersionStr,
    ) -> Result<FactorioInstance, FactorioError> {
        if let Some(installation) = self.get_factorio(version) {
            Ok(installation)
        } else {
            self.download_factorio(version).await?;
            self.get_factorio(version)
                .ok_or_else(|| FactorioError::InstallationNotFound(version))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, create_dir, create_dir_all};
    use tempfile::TempDir;
    use test_utils;

    impl FactorioInstallDir {
        pub(crate) fn test_dir() -> Self {
            Self::new_or_create(test_utils::test_factorio_installs_dir())
                .expect("Failed to create test directory")
        }
    }

    #[test]
    fn test_version_str() {
        let version = VersionStr::try_from("1.2.3").unwrap();
        assert_eq!(version.to_string(), "1.2.3");
        assert_eq!(version, VersionStr(1, 2, 3))
    }

    #[test]
    fn test_get_versions() -> Result<(), FactorioError> {
        let temp_dir = TempDir::new()?;
        let path = temp_dir.path();

        let make_installation = |name: &str| create_dir_all(path.join(name).join("factorio"));

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
}
