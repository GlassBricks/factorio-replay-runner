use anyhow::{Context, Result};
use async_process::{Child, Command};
use async_std::io::{BufReader, prelude::*};
use std::{
    fs::{File, create_dir_all},
    path::{Path, PathBuf},
    process::Stdio,
};

use crate::save_file::SaveFile;

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

    pub(crate) fn create_save_file(&self, file_name: &str) -> Result<File> {
        let mut saves_path = self.install_dir_abs.join("saves");
        create_dir_all(&saves_path)?;
        saves_path.push(file_name);
        Ok(File::create(saves_path)?)
    }

    pub fn read_save_file(&self, file_name: &str) -> Result<SaveFile<File>> {
        let saves_path = self.install_dir_abs.join("saves").join(file_name);
        let file = File::open(saves_path)?;
        Ok(SaveFile::new(file)?)
    }

    pub fn new_run_command(&self) -> Command {
        let path = self.install_dir_abs.join("bin/x64/factorio");
        Command::new(path)
    }
}

pub struct FactorioProcess {
    child: Child,
    stdout_reader: BufReader<async_process::ChildStdout>,
}

impl FactorioProcess {
    pub fn new(mut child: Child) -> Result<Self> {
        let Some(std_out) = child.stdout.take() else {
            anyhow::bail!("Child has no stdout");
        };
        let stdout_reader = BufReader::new(std_out);
        Ok(Self {
            child,
            stdout_reader,
        })
    }

    pub async fn read_all(&mut self) -> Result<String> {
        let mut output = String::new();
        self.stdout_reader.read_to_string(&mut output).await?;
        Ok(output)
    }
}

impl Drop for FactorioProcess {
    fn drop(&mut self) {
        self.child.kill().ok();
    }
}

impl FactorioInstallation {
    pub fn launch(&self, args: &[&str]) -> Result<FactorioProcess> {
        let child = self
            .new_run_command()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .args(args)
            .spawn()?;
        FactorioProcess::new(child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factorio_install_dir::FactorioInstallDir;

    impl FactorioInstallation {
        pub(crate) async fn test_installation() -> FactorioInstallation {
            FactorioInstallDir::test_dir()
                .get_or_download_factorio("2.0.45".try_into().unwrap())
                .await
                .expect("Failed to install factorio")
        }
    }

    #[async_std::test]
    async fn test_basic_run() -> Result<()> {
        let factorio = FactorioInstallation::test_installation().await;
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

    #[async_std::test]
    async fn test_read_all() -> Result<()> {
        let factorio = FactorioInstallation::test_installation().await;
        let mut process = factorio.launch(&["--version"])?;
        let output = process.read_all().await?;
        dbg!(output);
        anyhow::Ok(())
    }
}
