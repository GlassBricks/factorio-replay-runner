use std::{fs::File, io::Write, path::Path};

use anyhow::{Context, Result};
use log::info;
use replay_runner::{
    factorio_install_dir::FactorioInstallDir,
    replay_runner::{ReplayLog, RunResult, run_replay_with_rules},
    rules::RunRules,
    save_file::SaveFile,
};

pub async fn run_replay(
    install_dir: FactorioInstallDir,
    save_file: &mut SaveFile<File>,
    rules: &RunRules,
    output_path: &Path,
) -> RunResult {
    let log = run_replay_with_rules(&install_dir, save_file, rules).await?;
    write_replay_log(&log, output_path).context("Failed to write replay log")?;
    Ok(log)
}

fn write_replay_log(replay_log: &ReplayLog, output_path: &Path) -> Result<()> {
    let mut file = File::create(output_path)
        .with_context(|| format!("Failed to create output file: {}", output_path.display()))?;

    for msg in &replay_log.messages {
        writeln!(file, "[{}] {} {}", msg.msg_type, msg.time, msg.message)?
    }

    info!("Results written to: {}", output_path.display());
    Ok(())
}
