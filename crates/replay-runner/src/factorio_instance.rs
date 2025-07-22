use crate::save_file::SaveFile;
use anyhow::{Context, Result};
use async_process::{Child, Command};
use async_std::io::{BufReader, ReadExt};
use std::io;
use std::process::{Output, Stdio};
use std::{
    fs::{File, create_dir_all, remove_dir_all},
    path::{Path, PathBuf},
};

pub struct FactorioInstance {
    install_dir_abs: PathBuf,
}

impl FactorioInstance {
    pub fn new(install_dir: PathBuf) -> Result<Self> {
        let install_dir_abs = install_dir.canonicalize().with_context(|| {
            format!(
                "Failed to canonicalize install directory: {}",
                install_dir.display()
            )
        })?;
        Ok(FactorioInstance { install_dir_abs })
    }

    pub(crate) fn new_canonical(install_dir: PathBuf) -> Self {
        let install_dir_abs = install_dir.canonicalize().unwrap();
        FactorioInstance { install_dir_abs }
    }

    pub fn install_dir(&self) -> &Path {
        &self.install_dir_abs
    }

    pub fn create_save_file(&self, file_name: &str) -> Result<File> {
        let mut saves_path = self.install_dir_abs.join("saves");
        create_dir_all(&saves_path)?;
        saves_path.push(file_name);
        Ok(File::create(saves_path)?)
    }

    pub fn read_save_file(&self, file_name: &str) -> Result<SaveFile<File>> {
        let saves_path = self.install_dir_abs.join("saves").join(file_name);
        let file = File::open(saves_path)?;
        SaveFile::new(file)
    }

    pub fn delete_saves_dir(&self) -> Result<()> {
        let saves_path = self.install_dir_abs.join("saves");
        remove_dir_all(&saves_path)?;
        Ok(())
    }

    pub fn new_run_command(&self) -> Command {
        let path = self.install_dir_abs.join("bin/x64/factorio");
        Command::new(path)
    }

    pub fn spawn(&self, args: &[&str]) -> Result<FactorioProcess> {
        let child = self
            .new_run_command()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .args(args)
            .spawn()?;

        Ok(FactorioProcess::new(child))
    }

    pub fn output(&self, args: &[&str]) -> impl Future<Output = io::Result<Output>> {
        self.new_run_command().args(args).output()
    }
}

pub struct FactorioProcess {
    child: Child,
}

impl FactorioProcess {
    pub fn new(child: Child) -> Self {
        FactorioProcess { child }
    }
    pub fn stdout_reader(&mut self) -> Result<BufReader<&mut async_process::ChildStdout>> {
        self.child
            .stdout
            .as_mut()
            .map(BufReader::new)
            .ok_or_else(|| anyhow::anyhow!("Process has no stdout"))
    }

    pub async fn read_all(&mut self) -> Result<String> {
        let mut output = String::new();
        self.stdout_reader()
            .context("Process has no stdout")?
            .read_to_string(&mut output)
            .await?;
        Ok(output)
    }
}

impl Drop for FactorioProcess {
    fn drop(&mut self) {
        self.child.kill().ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{factorio_install_dir::FactorioInstallDir, save_file::TEST_VERSION};

    impl FactorioInstance {
        pub(crate) async fn get_test_installation() -> FactorioInstance {
            FactorioInstallDir::test_dir()
                .get_or_download_factorio(TEST_VERSION)
                .await
                .expect("Failed to install factorio")
        }
    }

    #[async_std::test]
    async fn test_spawn() -> Result<()> {
        let factorio = FactorioInstance::get_test_installation().await;
        let mut process = factorio.spawn(&["--version"])?;
        let output = process.read_all().await?;
        assert!(output.contains(&TEST_VERSION.to_string()));
        Ok(())
    }

    #[async_std::test]
    async fn test_output() -> Result<()> {
        let factorio = FactorioInstance::get_test_installation().await;
        let stdout = factorio.output(&["--version"]).await?.stdout;
        let output = String::from_utf8(stdout)?;
        assert!(output.contains(&TEST_VERSION.to_string()));
        Ok(())
    }
}
