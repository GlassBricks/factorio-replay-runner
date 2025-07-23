use anyhow::{Context, Result};
use clap::Parser;
use replay_runner::{
    factorio_install_dir::FactorioInstallDir, replay_runner::run_replay, save_file::SaveFile,
};
use replay_script::ReplayScripts;
use std::io::Write;
use std::{
    fs::File,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(name = "factorio-replay-cli")]
#[command(about = "Run Factorio replays with custom scripts and analyze the results")]
struct Args {
    /// Path to the Factorio save file (.zip)
    save_file: PathBuf,

    /// Path to the JSON rules file
    rules_file: PathBuf,

    /// Factorio installation directory (defaults to ./factorio_installs)
    #[arg(long, default_value = "./factorio_installs")]
    install_dir: PathBuf,

    /// Output file path (defaults to save file name with .txt extension)
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[async_std::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if !args.save_file.exists() {
        anyhow::bail!("Save file does not exist: {}", args.save_file.display());
    }
    if !args.rules_file.exists() {
        anyhow::bail!("Rules file does not exist: {}", args.rules_file.display());
    }

    let reader = File::open(&args.rules_file)
        .with_context(|| format!("Failed to open rules file: {}", args.rules_file.display()))?;

    let replay_scripts: ReplayScripts = serde_json::from_reader(reader).with_context(|| {
        format!(
            "Failed to parse rules file as JSON: {}",
            args.rules_file.display()
        )
    })?;

    println!("Running replay: {}", args.save_file.display());
    println!("Using rules from: {}", args.rules_file.display());
    println!("Factorio install dir: {}", args.install_dir.display());

    let output_path = args
        .output
        .unwrap_or_else(|| args.save_file.with_extension("txt"));

    let replay_log = cli_main(
        &args.save_file,
        &replay_scripts,
        &args.install_dir,
        &output_path,
    )
    .await?;

    if replay_log.exit_success {
        println!("Replay completed successfully!");
    } else {
        println!("Replay failed!");
    }

    println!("Results written to: {}", output_path.display());
    println!("Found {} log messages", replay_log.messages.len());

    let mut info_count = 0;
    let mut warn_count = 0;
    let mut error_count = 0;

    for msg in &replay_log.messages {
        match msg.msg_type {
            replay_script::MsgType::Info => info_count += 1,
            replay_script::MsgType::Warn => warn_count += 1,
            replay_script::MsgType::Error => error_count += 1,
        }
    }

    if error_count > 0 || warn_count > 0 || info_count > 0 {
        println!(
            "Summary: {} errors, {} warnings, {} info messages",
            error_count, warn_count, info_count
        );
    }

    Ok(())
}

async fn cli_main(
    save_file_path: &Path,
    replay_scripts: &ReplayScripts,
    install_dir_path: &Path,
    output_path: &Path,
) -> Result<replay_runner::replay_runner::ReplayLog> {
    let save_file_handle = File::open(save_file_path)
        .with_context(|| format!("Failed to open save file: {}", save_file_path.display()))?;

    let mut save_file = SaveFile::new(save_file_handle)
        .with_context(|| format!("Failed to load save file: {}", save_file_path.display()))?;

    let install_dir = FactorioInstallDir::new_or_create(install_dir_path).with_context(|| {
        format!(
            "Failed to create install directory: {}",
            install_dir_path.display()
        )
    })?;

    let replay_script = replay_scripts.to_string();

    let replay_log = run_replay(&install_dir, &mut save_file, &replay_script)
        .await
        .context("Failed to run replay")?;

    write_replay_log(&replay_log, output_path).context("Failed to write replay log")?;

    Ok(replay_log)
}

fn write_replay_log(
    replay_log: &replay_runner::replay_runner::ReplayLog,
    output_path: &Path,
) -> Result<()> {
    let mut file = File::create(output_path)
        .with_context(|| format!("Failed to create output file: {}", output_path.display()))?;

    for msg in &replay_log.messages {
        writeln!(file, "[{}] {} {}", msg.msg_type, msg.time, msg.message)
            .context("Failed to write message to output file")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use test_utils;

    use super::*;
    use std::fs;

    #[async_std::test]
    async fn test_run_replay_with_all_rules() -> Result<()> {
        let test_dir = test_utils::test_tmp_dir().join("cli_test");
        let fixtures_dir = test_utils::fixtures_dir();
        let install_dir_path = test_utils::test_factorio_installs_dir();

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }
        fs::create_dir_all(&test_dir)?;

        let test_save_path = fixtures_dir.join("TEST.zip");
        let replay_scripts = ReplayScripts::all_enabled();
        let output_path = test_dir.join("TEST.txt");

        let replay_log = cli_main(
            &test_save_path,
            &replay_scripts,
            &install_dir_path,
            &output_path,
        )
        .await?;

        assert!(replay_log.exit_success, "Replay should exit successfully");

        assert!(output_path.exists(), "Output file should be created");

        let output_content = fs::read_to_string(&output_path)?;

        let expected_log_path = fixtures_dir.join("TEST_expected.txt");
        let expected_content = fs::read_to_string(&expected_log_path).with_context(|| {
            format!(
                "Failed to read expected log file: {}",
                expected_log_path.display()
            )
        })?;

        assert_eq!(
            output_content.trim(),
            expected_content.trim(),
            "Log output should match expected content"
        );

        Ok(())
    }

    #[test]
    #[ignore]
    fn write_all_rules_to_fixtures() {
        let fixtures_dir = test_utils::fixtures_dir();
        let all_rules = ReplayScripts::all_enabled();

        let rules_json_path = fixtures_dir.join("all_rules.json");
        let rules_json = serde_json::to_string_pretty(&all_rules).unwrap();
        fs::write(rules_json_path, rules_json).unwrap();
    }
}
