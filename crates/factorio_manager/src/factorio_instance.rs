use crate::process_manager::GLOBAL_PROCESS_MANAGER;
use crate::save_file::SaveFile;
use anyhow::{Context, Result};
use async_process::{Child, Command};
use futures::io::{AsyncReadExt, BufReader};
use log::debug;
use std::io;
use std::process::{ExitStatus, Output, Stdio};
use std::{
    fs::{File, create_dir_all, remove_dir_all},
    path::{Path, PathBuf},
};

pub struct FactorioInstance {
    install_dir_abs: PathBuf,
}

impl FactorioInstance {
    pub fn new(install_dir: PathBuf) -> Result<Self> {
        let install_dir_abs = install_dir.canonicalize()?;
        Ok(FactorioInstance { install_dir_abs })
    }

    pub fn install_dir(&self) -> &Path {
        &self.install_dir_abs
    }

    pub fn create_save_file(&self, file_name: &str) -> Result<File> {
        let mut saves_path = self.install_dir_abs.join("saves");
        create_dir_all(&saves_path)?;
        saves_path.push(format!("{file_name}.zip"));
        Ok(File::create(saves_path)?)
    }

    pub fn read_save_file(&self, file_name: &str) -> Result<SaveFile<File>> {
        let saves_path = self.install_dir_abs.join("saves").join(file_name);
        let file = File::open(saves_path)?;
        SaveFile::new(file)
    }

    pub fn delete_saves_dir(&self) -> Result<()> {
        let saves_path = self.install_dir_abs.join("saves");
        if saves_path.exists() {
            remove_dir_all(&saves_path)?;
        }
        Ok(())
    }

    fn new_run_command(&self) -> Command {
        let path = self.install_dir_abs.join("bin/x64/factorio");
        Command::new(path)
    }

    pub fn spawn(&self, args: &[&str]) -> Result<FactorioProcess> {
        let mut cmd = self.new_run_command();
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).args(args);

        debug!("Launching: {:?}", cmd);

        let child = cmd.spawn().with_context(|| "Launching factorio")?;
        debug!("Spawned Factorio process with PID {}", child.id());
        Ok(FactorioProcess::new(child))
    }

    pub fn spawn_replay(&self, save_path: &Path) -> Result<FactorioProcess> {
        self.spawn(&["--run-replay", save_path.to_str().unwrap()])
    }

    pub async fn run_and_get_output(&self, args: &[&str]) -> Result<Output> {
        let mut cmd = self.new_run_command();
        cmd.args(args);
        debug!("Running: {:?}", &cmd);
        cmd.output().await.with_context(|| "Running factorio")
    }
}

pub struct FactorioProcess {
    child: Child,
}

impl FactorioProcess {
    pub fn new(child: Child) -> Self {
        GLOBAL_PROCESS_MANAGER.register(child.id());
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

    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.status().await
    }
}

impl Drop for FactorioProcess {
    fn drop(&mut self) {
        let pid = self.child.id();
        self.child.kill().ok();
        GLOBAL_PROCESS_MANAGER.unregister(pid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{factorio_install_dir::FactorioInstallDir, save_file::TEST_VERSION};

    impl FactorioInstance {
        pub(crate) async fn test_installation() -> FactorioInstance {
            FactorioInstallDir::test_dir()
                .get_or_download_factorio(TEST_VERSION)
                .await
                .expect("Failed to install factorio")
        }
    }

    #[tokio::test]
    async fn test_install_dir() -> Result<()> {
        let factorio = FactorioInstance::test_installation().await;
        let install_dir = factorio.install_dir();
        assert!(install_dir.exists());
        assert!(install_dir.join("bin/x64/factorio").exists());
        Ok(())
    }

    #[tokio::test]
    async fn test_create_and_read_save_file() -> Result<()> {
        let factorio = FactorioInstance::test_installation().await;

        factorio.create_save_file("test_save")?;

        let saves_path = factorio.install_dir().join("saves").join("test_save.zip");
        assert!(saves_path.exists());

        Ok(())
    }

    #[tokio::test]
    async fn test_delete_saves_dir() -> Result<()> {
        let factorio = FactorioInstance::test_installation().await;

        let _file = factorio.create_save_file("test_save")?;
        let saves_path = factorio.install_dir().join("saves");
        assert!(saves_path.exists());

        factorio.delete_saves_dir()?;
        assert!(!saves_path.exists());

        Ok(())
    }

    #[tokio::test]
    async fn test_spawn() -> Result<()> {
        let factorio = FactorioInstance::test_installation().await;
        let mut process = factorio.spawn(&["--version"])?;
        let output = process.read_all().await?;
        assert!(output.contains(&TEST_VERSION.to_string()));
        Ok(())
    }

    #[tokio::test]
    async fn test_output() -> Result<()> {
        let factorio = FactorioInstance::test_installation().await;
        let stdout = factorio.run_and_get_output(&["--version"]).await?.stdout;
        let output = String::from_utf8(stdout)?;
        assert!(output.contains(&TEST_VERSION.to_string()));
        Ok(())
    }
}
