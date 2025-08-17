use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use log::{error, info};
use replay_runner::{
    factorio_install_dir::FactorioInstallDir,
    replay_runner::{ReplayLog, RunResult},
    rules::RunRules,
    save_file::SaveFile,
};
use replay_script::MsgType;
use run::run_replay;
use std::{
    fs::File,
    path::{Path, PathBuf},
};

mod run;

#[derive(Parser)]
#[command(name = "factorio-replay-cli")]
#[command(about = "Run Factorio replays with custom scripts and analyze the results")]
struct CliArgs {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run(RunReplayOnFileArgs),
    RunSrc(RunReplayFromSrcArgs),
}

#[derive(Args)]
struct RunReplayOnFileArgs {
    /// Factorio save file
    save_file: PathBuf,

    /// Rules file (json/yaml)
    rules_file: PathBuf,

    /// Factorio installations directory (defaults to ./factorio_installs)
    /// Installs will created at {install_dir}/{version}/
    #[arg(long, default_value = "./factorio_installs")]
    install_dir: PathBuf,

    /// Output file path; defaults to save file name with .txt extension
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[derive(Args)]
struct RunReplayFromSrcArgs {
    /// Rules directory
    rules_dir: PathBuf,

    /// Factorio installations directory (defaults to ./factorio_installs)
    /// Installs will created at {install_dir}/{version}/
    #[arg(long, default_value = "./factorio_installs")]
    install_dir: PathBuf,

    /// Output path; defaults to ./src_runs
    /// Files will be written to {output_dir}/{run_id}/
    ///     {save_name}.zip
    ///     {log}.txt
    #[arg(short, long)]
    output: Option<PathBuf>,
}

fn init_logger() -> Result<()> {
    use simplelog::*;
    CombinedLogger::init(vec![
        TermLogger::new(
            LevelFilter::Info,
            Config::default(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ),
        WriteLogger::new(
            LevelFilter::Info,
            Config::default(),
            File::create("factorio-replay-cli.log").unwrap(),
        ),
    ])?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logger()?;
    let args = CliArgs::parse();

    let exit_code = match args.command {
        Commands::Run(sub_args) => cli_run_file(sub_args).await,
        Commands::RunSrc(_) => todo!(),
    }?;

    std::process::exit(exit_code);
}

/// Exit codes:
/// 0: Success
/// 1: Warning
/// 2: Error
/// 10: replay run error
async fn cli_run_file(args: RunReplayOnFileArgs) -> Result<i32> {
    let RunReplayOnFileArgs {
        save_file,
        rules_file,
        install_dir,
        output,
    } = args;
    let output_path = output.unwrap_or_else(|| save_file.with_extension("log"));
    let result = run_replay_from_paths(&save_file, &rules_file, &install_dir, &output_path).await?;
    let exit_code = match result {
        RunResult::PreRunCheckFailed(err) => {
            error!("Pre-run check failed: {}", err);
            1
        }
        RunResult::ReplayRan(replay_log) => {
            if replay_log.exit_success {
                info!("Replay completed successfully!");
            } else {
                info!("Replay failed!");
            }
            let result_code = get_result(&replay_log, &output_path);
            if replay_log.exit_success {
                result_code
            } else {
                10
            }
        }
    };

    Ok(exit_code)
}

async fn run_replay_from_paths(
    save_file: &Path,
    rules_file: &Path,
    install_dir: &Path,
    output: &Path,
) -> Result<RunResult> {
    anyhow::ensure!(
        save_file.exists(),
        "Save file does not exist: {}",
        save_file.display()
    );
    anyhow::ensure!(
        rules_file.exists(),
        "Rules file does not exist: {}",
        rules_file.display()
    );
    anyhow::ensure!(
        install_dir.exists(),
        "Factorio install dir does not exist: {}",
        install_dir.display()
    );

    info!("Running replay: {}", save_file.display());
    info!("Using rules from: {}", rules_file.display());
    info!("Factorio install dir: {}", install_dir.display());

    let (install_dir, mut save_file, rules) =
        load_replay_inputs(save_file, rules_file, install_dir).await?;
    run_replay(install_dir, &mut save_file, &rules, &output).await
}

fn get_result(replay_log: &ReplayLog, replay_log_path: &Path) -> i32 {
    info!("Results written to: {}", replay_log_path.display());

    if replay_log
        .messages
        .iter()
        .any(|msg| msg.msg_type == MsgType::Error)
    {
        return 1;
    }
    if replay_log
        .messages
        .iter()
        .any(|msg| msg.msg_type == MsgType::Warn)
    {
        return 2;
    }
    return 0;
}

async fn load_replay_inputs(
    save_file_path: &Path,
    rules_file_path: &Path,
    install_dir_path: &Path,
) -> Result<(FactorioInstallDir, SaveFile<File>, RunRules)> {
    let save_file_handle = File::open(save_file_path)
        .with_context(|| format!("Failed to open save file: {}", save_file_path.display()))?;

    let save_file = SaveFile::new(save_file_handle)
        .with_context(|| format!("Failed to load save file: {}", save_file_path.display()))?;

    let install_dir = FactorioInstallDir::new_or_create(install_dir_path).with_context(|| {
        format!(
            "Failed to create install directory: {}",
            install_dir_path.display()
        )
    })?;

    let reader = File::open(rules_file_path)
        .with_context(|| format!("Failed to open rules file: {}", rules_file_path.display()))?;

    let rules: RunRules = serde_yaml::from_reader(reader).with_context(|| {
        format!(
            "Failed to parse rules file as YAML: {}",
            rules_file_path.display()
        )
    })?;

    Ok((install_dir, save_file, rules))
}

#[cfg(test)]
mod tests {
    use replay_runner::expected_mods::ExpectedMods;
    use replay_script::ReplayScripts;
    use test_utils;

    use super::*;
    use std::fs;

    fn write_all_rules_to_fixtures() {
        let fixtures_dir = test_utils::fixtures_dir();
        let all_scripts = ReplayScripts::all_enabled();
        let test_all_rules = RunRules {
            expected_mods: ExpectedMods::SpaceAge,
            replay_checks: all_scripts,
        };

        let rules_yaml = serde_yaml::to_string(&test_all_rules).unwrap();
        fs::write(fixtures_dir.join("all_checks.yaml"), rules_yaml).unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_run_file() -> Result<()> {
        write_all_rules_to_fixtures();

        let test_dir = test_utils::test_tmp_dir().join("cli_test");
        let fixtures_dir = test_utils::fixtures_dir();
        let install_dir_path = test_utils::test_factorio_installs_dir();

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }
        fs::create_dir_all(&test_dir)?;

        let test_save_path = fixtures_dir.join("TEST.zip");
        let rules_file_path = fixtures_dir.join("all_checks.yaml");
        let output_path = test_dir.join("TEST.txt");

        let result = run_replay_from_paths(
            &test_save_path,
            &rules_file_path,
            &install_dir_path,
            &output_path,
        )
        .await?;

        let replay_log = match result {
            RunResult::ReplayRan(log) => log,
            RunResult::PreRunCheckFailed(err) => {
                panic!("Pre-run check failed: {}", err);
            }
        };

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
}
