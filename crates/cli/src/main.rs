use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use log::{error, info};
use replay_runner::{
    factorio_install_dir::FactorioInstallDir,
    replay_runner::{ReplayLog, RunResult},
    rules::{GameRules, RunRules},
    save_file::SaveFile,
};
use replay_script::MsgType;
use run::run_replay;
use run_downloader::{
    FileDownloader,
    services::{dropbox::DropboxService, gdrive::GoogleDriveService},
};
use src_integration::run_replay_from_src_run;
use std::{
    fmt::Display,
    fs::File,
    path::{Path, PathBuf},
};

mod run;
mod src_integration;

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

    /// RUN Rules file (json/yaml)
    run_rules_file: PathBuf,

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
    /// Run id
    run_id: String,

    /// GAME rules file (json/yaml)
    game_rules_file: PathBuf,

    /// Factorio installations directory (defaults to ./factorio_installs)
    /// Installs will created at {install_dir}/{version}/
    #[arg(long, default_value = "./factorio_installs")]
    install_dir: PathBuf,

    /// Output path; defaults to ./src_runs
    /// Files will be written to {output_dir}/{run_id}/
    ///     {save_name}.zip
    ///     {log}.txt
    #[arg(short, long, default_value = "./src_runs")]
    output_dir: PathBuf,
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
        Commands::RunSrc(sub_args) => cli_run_src(sub_args).await,
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
        run_rules_file: rules_file,
        install_dir,
        output,
    } = args;
    let output_path = output.unwrap_or_else(|| save_file.with_extension("log"));

    let install_dir = load_install_dir(&install_dir).await?;
    let mut save_file = load_save_file(&save_file).await?;
    let rules = load_run_rules(&rules_file).await?;
    let result = run_replay(install_dir, &mut save_file, &rules, &output_path).await;
    log_result(&result);
    Ok(result_to_exit_code(&result))
}

async fn run_replay_from_paths(
    save_file: &Path,
    rules_file: &Path,
    install_dir: &Path,
    output: &Path,
) -> RunResult {
    let install_dir = load_install_dir(install_dir).await?;
    let mut save_file = load_save_file(save_file).await?;
    let rules = load_run_rules(rules_file).await?;

    run_replay(install_dir, &mut save_file, &rules, &output).await
}

async fn cli_run_src(args: RunReplayFromSrcArgs) -> Result<i32> {
    let RunReplayFromSrcArgs {
        run_id,
        game_rules_file,
        install_dir,
        output_dir,
    } = args;

    let install_dir = load_install_dir(&install_dir).await?;
    let rules = load_game_rules(&game_rules_file).await?;

    let mut downloader = create_file_downloader().await?;
    let result =
        run_replay_from_src_run(&mut downloader, &run_id, &install_dir, &rules, &output_dir).await;

    log_result(&result);
    Ok(result_to_exit_code(&result))
}

async fn create_file_downloader() -> Result<FileDownloader> {
    Ok(FileDownloader::builder()
        .add_service(GoogleDriveService::from_env().await?)
        .add_service(DropboxService::from_env().await?)
        .build())
}

async fn load_install_dir(install_dir_path: &Path) -> Result<FactorioInstallDir> {
    FactorioInstallDir::new_or_create(install_dir_path).with_context(|| {
        format!(
            "Failed to create install directory: {}",
            install_dir_path.display()
        )
    })
}

async fn load_save_file(save_file_path: &Path) -> Result<SaveFile<File>> {
    let save_file_handle = File::open(save_file_path)?;
    SaveFile::new(save_file_handle)
}

async fn load_run_rules(rules_file_path: &Path) -> Result<RunRules> {
    serde_yaml::from_reader(File::open(rules_file_path)?).with_context(|| "failed to load rules")
}

async fn load_game_rules(game_rules_file_path: &Path) -> Result<GameRules> {
    let reader = File::open(game_rules_file_path).with_context(|| {
        format!(
            "Failed to open game rules file: {}",
            game_rules_file_path.display()
        )
    })?;

    serde_yaml::from_reader(reader).with_context(|| {
        format!(
            "Failed to parse game rules file as YAML: {}",
            game_rules_file_path.display()
        )
    })
}

fn log_result(result: &Result<ReplayLog, impl Display>) {
    match result {
        Ok(replay_log) => {
            if replay_log.exit_success {
                info!("Replay completed successfully!");
            } else {
                info!("Replay failed!");
            }
        }
        Err(err) => {
            error!("{err}");
        }
    }
}

fn result_to_exit_code(result: &Result<ReplayLog, impl Display>) -> i32 {
    match result {
        Err(err) => {
            error!("{err}");
            1
        }
        Ok(replay_log) => {
            if !replay_log.exit_success {
                10
            } else if replay_log
                .messages
                .iter()
                .any(|msg| msg.msg_type == MsgType::Error)
            {
                1
            } else if replay_log
                .messages
                .iter()
                .any(|msg| msg.msg_type == MsgType::Warn)
            {
                2
            } else {
                0
            }
        }
    }
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

        assert!(result.exit_success, "Replay should exit successfully");

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
