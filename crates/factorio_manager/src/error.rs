use std::io;
use thiserror::Error;

use crate::factorio_install_dir::VersionStr;

#[derive(Debug, Error)]
pub enum FactorioError {
    #[error("Invalid save file: {0}")]
    InvalidSaveFile(#[source] anyhow::Error),

    #[error("Invalid version string: {0}")]
    InvalidVersion(#[source] anyhow::Error),

    #[error("Factorio version {version} is not supported")]
    VersionTooOld { version: VersionStr },

    #[error("Mod mismatch. Missing: {missing_mods:?}, Extra: {extra_mods:?}")]
    ModMismatch {
        missing_mods: Vec<String>,
        extra_mods: Vec<String>,
    },

    #[error("Failed to inject replay script: {0}")]
    ScriptInjectionFailed(#[source] anyhow::Error),

    #[error("Failed to download Factorio {version}")]
    FactorioDownloadFailed {
        version: VersionStr,
        #[source]
        source: anyhow::Error,
    },

    #[error("Failed to extract Factorio: {0}")]
    ExtractionFailed(#[source] anyhow::Error),

    #[error("Factorio installation not found for version {0}")]
    InstallationNotFound(VersionStr),

    #[error("Install directory error: {0}")]
    InstallDirError(#[source] anyhow::Error),

    #[error("Failed to spawn Factorio process: {0}")]
    ProcessSpawnFailed(#[source] io::Error),

    #[error("Failed to read mod information: {0}")]
    ModInfoReadFailed(#[source] anyhow::Error),

    #[error("Factorio process exited unsuccessfully with exit code: {}", exit_code.map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string()))]
    ProcessExitedUnsuccessfully { exit_code: Option<i32> },

    #[error("Replay timeout: no log messages produced for 5 minutes")]
    ReplayTimeout,

    #[error("IO error: {0}")]
    IoError(#[from] io::Error),
}
