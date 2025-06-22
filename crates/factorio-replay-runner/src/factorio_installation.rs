use anyhow::{Context, Result};
use async_process::Command;
use std::path::{Path, PathBuf};

pub struct FactorioInstallation {
    install_dir_abs: PathBuf,
}

impl FactorioInstallation {
    pub fn new(install_dir: PathBuf) -> Result<Self> {
        let install_dir_abs = install_dir.canonicalize().with_context(|| {
            format!(
                "Failed to canonicalize install directory: {}",
                install_dir.display()
            )
        })?;
        Ok(FactorioInstallation { install_dir_abs })
    }

    pub(crate) fn new_canonical(install_dir: PathBuf) -> Self {
        let install_dir_abs = install_dir.canonicalize().unwrap();
        FactorioInstallation { install_dir_abs }
    }

    pub fn install_dir(&self) -> &Path {
        &self.install_dir_abs
    }

    pub(crate) fn new_run_command(&self) -> Command {
        let path = self.install_dir_abs.join("bin/x64/factorio");
        Command::new(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factorio_install_dir::FactorioInstallDir;

    #[tokio::test]
    async fn test_run() -> Result<()> {
        let factorio = FactorioInstallDir::test_dir()
            .get_or_download_factorio("2.0.45".try_into().unwrap())
            .await?;
        let result = factorio
            .new_run_command()
            .args(["--version"])
            .status()
            .await
            .context("Failed to execute factorio command")?;
        anyhow::ensure!(
            result.success(),
            "Factorio command failed with status: {}",
            result
        );
        Ok(())
    }
}
